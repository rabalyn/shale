//! What the window does with the asks of the Data usage dialog and with a finished clear or
//! prune (USE-005, USE-010, USE-015, USE-017, USE-020, USE-023).

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
