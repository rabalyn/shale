//! What the window does with the asks of the Data usage dialog and with a finished clear or
//! prune (USE-005, USE-010, USE-015, USE-017, USE-020, USE-023, USE-026).

#![cfg(test)]

use super::*;
use crate::app::tests::test_app;
use crate::data_usage::Freed;
use eframe::egui;

fn ok(id: CacheId, kind: Kind, entries: u64) -> Finished {
    Finished { id, kind, result: Ok(Freed { bytes: 10 * entries, entries }) }
}

fn said(app: &ShaleApp) -> Vec<&str> {
    app.toasts.iter().map(|(t, _)| t.as_str()).collect()
}

// spec: USE-005, USE-020, USE-023
#[test]
fn refresh_counts_again_and_a_changed_setting_prunes_that_cache_after_it_settled() {
    let (mut app, d) = test_app("usage-ask");
    app.ctx = Some(egui::Context::default());
    app.usage.saved = Some(Err("old count".into()));
    assert!(!app.usage_ask(Ask::Refresh), "the dialog stays open");
    assert!(app.usage.saved.is_none(), "counting again: the old numbers are gone until the new ones are in");
    let start = Instant::now();
    while app.usage.saved.is_none() {
        app.usage_tick();
        assert!(start.elapsed() < Duration::from_secs(40), "the count did not end");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(matches!(app.usage.saved, Some(Ok(_))));
    // a changed retention setting: nothing at once, the prune when the value settled
    assert!(!app.usage_ask(Ask::Changed(CacheId::DebugLog)), "the dialog stays open");
    assert!(app.usage.due(Instant::now(), 0).is_empty(), "not while the value is still being changed");
    assert_eq!(app.usage.due(Instant::now() + crate::data_usage::SETTLE, 0), [CacheId::DebugLog]);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: USE-010
#[test]
fn a_finished_picture_cache_clear_or_prune_makes_the_window_forget_the_pictures_it_knows() {
    let (mut app, d) = test_app("usage-pictures");
    let ctx = egui::Context::default();
    let url = "http://example.invalid/x.png";
    for kind in [Kind::Clear, Kind::Prune] {
        let images = crate::social::images::ImageCache::new(&ctx, d.join("pictures"));
        assert_eq!(images.get(url), None);
        let start = Instant::now();
        while !images.failed(url) {
            assert!(start.elapsed() < Duration::from_secs(40), "the address was not refused");
            std::thread::sleep(Duration::from_millis(10));
        }
        app.social.images = Some(images);
        app.usage_finished(ok(CacheId::SocialPictures, kind, 0));
        assert!(!app.social.images.as_ref().unwrap().failed(url), "{kind:?}: the picture is asked for again when it is shown");
    }
    let _ = std::fs::remove_dir_all(&d);
}

// spec: USE-015
#[test]
fn only_a_clear_of_the_tag_words_makes_the_tag_learner_forget() {
    let (mut app, d) = test_app("usage-tags");
    app.tag_learner.generation = 0;
    app.usage_finished(ok(CacheId::TagWords, Kind::Prune, 3));
    assert_eq!(app.tag_learner.generation, 0, "a prune leaves what was learned");
    app.usage_finished(ok(CacheId::TagWords, Kind::Clear, 3));
    assert_eq!(app.tag_learner.generation, u64::MAX, "a clear: learning starts again");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: USE-017
#[test]
fn a_finished_undo_prune_writes_again_the_histories_that_were_deleted_while_it_ran() {
    let (mut app, d) = test_app("usage-undo");
    app.undo.open_vault(&d, crate::app::unix_now());
    app.undo.of(Path::new("Ideas.md")).checkpoint("one");
    app.undo.of(Path::new("Ideas.md")).checkpoint("one two");
    app.undo.flush_all();
    assert!(!app.undo.has_pending());
    assert_eq!(crate::undo::stats_of_files(&d).entries, 1);
    // the job removed the file of a history that is still loaded and has steps
    for e in std::fs::read_dir(d.join(".shale/undo")).unwrap().flatten() {
        std::fs::remove_file(e.path()).unwrap();
    }
    assert_eq!(crate::undo::stats_of_files(&d).entries, 0);
    app.usage_finished(ok(CacheId::UndoHistories, Kind::Prune, 0));
    assert!(app.undo.has_pending(), "the history has to be written again");
    app.undo.flush_all();
    assert_eq!(crate::undo::stats_of_files(&d).entries, 1, "its file is back");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: USE-007, USE-023
#[test]
fn a_finished_job_says_so_always_for_a_clear_and_a_failure_and_for_a_prune_only_when_it_removed_something() {
    let (mut app, d) = test_app("usage-toasts");
    app.usage_finished(ok(CacheId::DebugLog, Kind::Prune, 0));
    assert!(said(&app).is_empty(), "a prune that removed nothing is silent: {:?}", said(&app));
    app.usage_finished(ok(CacheId::DebugLog, Kind::Prune, 2));
    assert_eq!(said(&app), ["Input debug log: cleanup removed 2 entries (20 bytes)."]);
    app.toasts.clear();
    app.usage_finished(ok(CacheId::DebugLog, Kind::Clear, 0));
    assert_eq!(said(&app), ["Input debug log cleared: 0 bytes freed."], "a clear always says so");
    app.toasts.clear();
    app.usage_finished(Finished { id: CacheId::DebugLog, kind: Kind::Prune, result: Err("busy".into()) });
    assert_eq!(said(&app), ["Input debug log: the cleanup failed: busy"], "a failure always says so");
    app.toasts.clear();
    app.usage_finished(Finished { id: CacheId::DebugLog, kind: Kind::Clear, result: Err("busy".into()) });
    assert_eq!(said(&app), ["Input debug log: could not clear: busy"]);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: USE-026, MEDIA-022
#[test]
fn the_dialog_counts_the_outside_media_of_the_open_vault_and_again_after_a_refresh() {
    let (mut app, d) = test_app("usage-linked");
    app.ctx = Some(egui::Context::default());
    let videos = d.join("Videos-outside");
    std::fs::create_dir_all(&videos).unwrap();
    std::fs::write(videos.join("a.mp4"), vec![0u8; 2_000]).unwrap();
    app.cfg.media_libraries = vec![crate::media::outside::Library { name: "Videos".into(), path: videos.clone() }];
    app.vault.as_mut().unwrap().set_libraries(&app.cfg.media_libraries);
    std::fs::write(d.join("Clips.md"), "![[@Videos/a.mp4]]\n\n[[@Videos/gone.mp4]]\n").unwrap();
    app.vault.as_mut().unwrap().rescan();
    let wait = |app: &mut ShaleApp| {
        let start = Instant::now();
        while app.usage.saved.is_none() {
            app.usage_tick();
            assert!(start.elapsed() < Duration::from_secs(40), "the count did not end");
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    app.open_data_usage();
    assert_eq!(app.usage.places.linked.links.len(), 2, "the window hands the links of its index to the job");
    wait(&mut app);
    let line = app.usage.saved.clone().unwrap().unwrap().linked.expect("a line for the outside media");
    assert_eq!((line.files, line.bytes, line.problem.as_deref()), (1, 2_000, Some("1 not found or not set up on this device")));
    // the file comes back, the note links a second one: Refresh counts what the vault links now
    std::fs::write(videos.join("gone.mp4"), vec![0u8; 100]).unwrap();
    std::fs::write(videos.join("b.mp3"), vec![0u8; 10]).unwrap();
    std::fs::write(d.join("More.md"), "[[@Videos/b.mp3]]\n").unwrap();
    app.vault.as_mut().unwrap().rescan();
    app.usage_ask(Ask::Refresh);
    wait(&mut app);
    let line = app.usage.saved.clone().unwrap().unwrap().linked.unwrap();
    assert_eq!((line.files, line.bytes, line.problem), (3, 2_110, None));
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&videos);
}

// spec: USE-012, JOB-016
#[test]
fn job_016_the_rebuild_of_the_mail_index_is_a_task_that_cancel_stops_and_a_later_rebuild_completes() {
    use crate::mail::ui::MAIL_REBUILD;
    let (mut app, d) = test_app("usage-mail-task");
    app.ctx = Some(egui::Context::default());
    let cfg = app.cfg.clone();
    let start = Instant::now();
    while app.mail.ready_store().is_none() {
        app.mail.tick(&cfg, true);
        assert!(start.elapsed() < Duration::from_secs(40), "the store opens");
        std::thread::sleep(Duration::from_millis(10));
    }
    let store = app.mail.ready_store().unwrap();
    let inbox = store.ensure_folder("acct1", "A", "INBOX", Some("/"), None, true).unwrap();
    for uid in 1..=3u32 {
        let raw = format!("From: a@x.de\r\nSubject: s{uid}\r\nMessage-ID: <s{uid}@x>\r\n\r\nbody {uid}\r\n");
        let m = store.save_message(&inbox, 7, uid, raw.as_bytes(), "", None).unwrap();
        store.commit(vec![crate::mail::db::Op::message(m)]).unwrap();
    }
    let count = |store: &crate::mail::store::MailStore| store.reader().unwrap().count_messages().unwrap();
    assert_eq!(count(&store), 3);
    let wait_for_the_end = |app: &mut ShaleApp| {
        let start = Instant::now();
        while app.usage.running(CacheId::MailIndex) {
            app.usage_tick();
            assert!(start.elapsed() < Duration::from_secs(40), "the rebuild ends");
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    // the writer waits at a gate, so the rebuild is still queued while the registry is looked at
    assert!(app.busy.info(&MAIL_REBUILD).is_none());
    let gate = store.hold_writer();
    app.usage_start(CacheId::MailIndex, Kind::Clear);
    let task = app.busy.info(&MAIL_REBUILD).expect("the rebuild is listed while it runs");
    assert_eq!(task.label, "Rebuilding the mail index…");
    assert!(task.cancellable && !task.cancelling);
    assert!(app.busy.list().iter().any(|t| t.id == task.id), "the indicator lists it");
    // a second rebuild does not start: the task is the one already there
    app.usage_start(CacheId::MailIndex, Kind::Clear);
    assert_eq!(app.busy.list().len(), 1);
    // Cancel: the task stays until the job ends, then it is gone and the store is back
    assert!(app.busy.cancel(task.id));
    assert!(app.busy.info(&MAIL_REBUILD).unwrap().cancelling);
    gate.send(()).unwrap();
    wait_for_the_end(&mut app);
    assert!(app.busy.info(&MAIL_REBUILD).is_none(), "the task ended with the job");
    assert!(app.mail.ready_store().is_some(), "the store is given back");
    assert!(said(&app).iter().any(|t| t.starts_with("Mail index: the rebuild was stopped")), "{:?}", said(&app));
    assert_eq!(count(&store), 3, "stopped before any row went: the index is as it was");
    crate::mail::db::quick_check(&store.index_path()).expect("the index passes the integrity check");
    // a rebuild after Cancel is a new task with a fresh token and completes
    app.usage_start(CacheId::MailIndex, Kind::Clear);
    assert!(!app.busy.info(&MAIL_REBUILD).is_some_and(|t| t.cancelling));
    wait_for_the_end(&mut app);
    assert!(app.busy.info(&MAIL_REBUILD).is_none());
    assert_eq!(count(&store), 3);
    assert!(said(&app).iter().any(|t| t.starts_with("Mail index rebuilt from the message files: 3")), "{:?}", said(&app));
    let _ = std::fs::remove_dir_all(&d);
}
