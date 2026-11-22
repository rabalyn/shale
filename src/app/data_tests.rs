//! Data safety in the app (ADR-0003): unreadable notes, outside changes, conflict copies, failed
//! saves. The rules are DATA-002, DATA-003, DATA-008, DATA-009 and VAULT-016.

use super::*;

fn app_in(tag: &str, files: &[(&str, &[u8])]) -> (ShaleApp, PathBuf) {
    let d = std::env::temp_dir().join(format!("shale-data-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for (name, bytes) in files {
        let p = d.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        crate::fsutil::write_atomic(&p, bytes).unwrap();
    }
    let app = ShaleApp::build(Options { screenshot: None, vault: Some(d.clone()), view: None, keys: None }, false);
    (app, d)
}

fn read(d: &Path, name: &str) -> String {
    crate::fsutil::read_to_string(d.join(name)).unwrap()
}

/// Files of the vault folder whose name starts with `prefix`.
fn names(d: &Path, prefix: &str) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with(prefix)).collect();
    v.sort();
    v
}

/// Type into the buffer the way the editor does: the text changes and the note is dirty.
fn type_text(app: &mut ShaleApp, rel: &str, text: &str) {
    let doc = app.notes.get_mut(Path::new(rel)).unwrap();
    doc.text = text.to_string();
    doc.dirty = true;
    doc.last_edit = Instant::now() - Duration::from_secs(5);
    app.vault.as_mut().unwrap().set_note_text(Path::new(rel), text);
}

fn toasted(app: &ShaleApp, part: &str) -> bool {
    app.toasts.iter().any(|(t, _)| t.contains(part))
}

/// Type into the buffer of `rel` as `type_text` does, but the last edit was `ago` ago.
fn edited_ago(app: &mut ShaleApp, rel: &str, text: &str, ago: Duration) {
    type_text(app, rel, text);
    app.notes.get_mut(Path::new(rel)).unwrap().last_edit = Instant::now() - ago;
}

// spec: DATA-001
#[test]
fn data_001_a_note_is_saved_by_the_autosave_only_after_0_8_s_without_an_edit() {
    let (mut app, d) = app_in("data-001-autosave", &[("N.md", b"base\n")]);
    app.open(Path::new("N.md"), false);
    // the clock is the age of the last edit: well before 0.8 s nothing is written, well after it is
    edited_ago(&mut app, "N.md", "just typed\n", Duration::ZERO);
    app.autosave();
    assert_eq!(read(&d, "N.md"), "base\n", "an edit that has just been made is not saved yet");
    edited_ago(&mut app, "N.md", "a moment ago\n", Duration::from_millis(600));
    app.autosave();
    assert_eq!(read(&d, "N.md"), "base\n", "0.6 s are not enough");
    assert!(app.notes[Path::new("N.md")].dirty);
    edited_ago(&mut app, "N.md", "a while ago\n", Duration::from_millis(1000));
    app.autosave();
    assert_eq!(read(&d, "N.md"), "a while ago\n", "after more than 0.8 s the note is saved");
    assert!(!app.notes[Path::new("N.md")].dirty);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: DATA-001
#[test]
fn data_001_a_note_is_saved_at_once_when_its_tab_closes_and_at_exit() {
    let (mut app, d) = app_in("data-001-close", &[("N.md", b"base\n"), ("M.md", b"base\n")]);
    app.open(Path::new("N.md"), false);
    app.open(Path::new("M.md"), true);
    // edited just now: the autosave would not write it
    edited_ago(&mut app, "N.md", "closed with its tab\n", Duration::ZERO);
    app.autosave();
    assert_eq!(read(&d, "N.md"), "base\n");
    let tab = app.tabbar.tabs.iter().position(|t| t.view == View::Note(PathBuf::from("N.md"))).unwrap();
    app.close_tab(tab);
    assert_eq!(read(&d, "N.md"), "closed with its tab\n", "closing the tab saves");
    // an edit just made to the note that stays open waits for the autosave, until the exit
    edited_ago(&mut app, "M.md", "left open at exit\n", Duration::ZERO);
    app.autosave();
    assert_ne!(read(&d, "M.md"), "left open at exit\n");
    eframe::App::on_exit(&mut app);
    assert_eq!(read(&d, "M.md"), "left open at exit\n", "the exit saves");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: DATA-008
#[test]
fn a_note_that_cannot_be_read_opens_read_only_and_is_never_overwritten() {
    let bad: &[u8] = b"Caf\xe9 \xff\xfe broken bytes\n";
    let (mut app, d) = app_in("unreadable", &[("Bad.md", bad), ("Good.md", b"fine\n")]);
    let v = app.vault.as_ref().unwrap();
    assert!(v.notes[Path::new("Bad.md")].unreadable.is_some(), "the index knows it could not be read");
    assert!(v.notes[Path::new("Good.md")].unreadable.is_none());
    app.open(Path::new("Bad.md"), false);
    let doc = &app.notes[Path::new("Bad.md")];
    assert!(doc.unreadable.as_deref().unwrap_or("").contains("Bad.md"), "the reason names the file: {:?}", doc.unreadable);
    assert_eq!(doc.text, "");

    // a keystroke that got into the buffer anyway, then autosave, then every other way to save
    type_text(&mut app, "Bad.md", "x");
    app.autosave();
    app.flush_all();
    app.save_note(Path::new("Bad.md"));
    app.rescan();
    assert_eq!(std::fs::read(d.join("Bad.md")).unwrap(), bad, "the original bytes are untouched");
    assert_eq!(names(&d, "Bad"), vec!["Bad.md"], "and nothing else was made");

    // rename keeps the bytes; checkbox toggling and the link lists refuse
    app.rename(Path::new("Bad.md"), "Still bad").unwrap();
    assert_eq!(std::fs::read(d.join("Still bad.md")).unwrap(), bad);
    assert!(app.vault.as_mut().unwrap().edit_note(Path::new("Still bad.md"), |_| Some("x".into())).is_err());
    assert_eq!(std::fs::read(d.join("Still bad.md")).unwrap(), bad);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: DATA-008
#[test]
fn a_note_that_becomes_readable_again_is_shown() {
    let (mut app, d) = app_in("recover", &[("N.md", b"\xff\xfe")]);
    app.open(Path::new("N.md"), false);
    assert!(app.notes[Path::new("N.md")].unreadable.is_some());
    crate::fsutil::write_atomic(&d.join("N.md"), b"now text\n").unwrap();
    app.rescan();
    let doc = &app.notes[Path::new("N.md")];
    assert!(doc.unreadable.is_none());
    assert_eq!(doc.text, "now text\n");
    // and the other way: the other program writes bytes that are not text
    crate::fsutil::write_atomic(&d.join("N.md"), b"\xff\xff").unwrap();
    app.rescan();
    assert!(app.notes[Path::new("N.md")].unreadable.is_some());
    assert_eq!(app.notes[Path::new("N.md")].text, "");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: DATA-002
#[test]
fn an_open_note_without_edits_follows_the_file() {
    let (mut app, d) = app_in("follow", &[("N.md", b"one\n")]);
    app.open(Path::new("N.md"), false);
    crate::fsutil::write_atomic(&d.join("N.md"), b"two\n").unwrap();
    app.rescan();
    assert_eq!(app.notes[Path::new("N.md")].text, "two\n");
    assert!(!app.notes[Path::new("N.md")].dirty);
    // the next edit is saved over the reloaded text without a conflict
    type_text(&mut app, "N.md", "three\n");
    app.autosave();
    assert_eq!(read(&d, "N.md"), "three\n");
    assert_eq!(names(&d, "N"), vec!["N.md"]);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: DATA-002
#[test]
fn an_open_drawing_without_edits_is_reloaded_and_one_with_edits_is_not() {
    let scene = |n: usize| {
        let mut s = crate::drawing::Scene::parse(crate::drawing::SAMPLE_SCENE).unwrap();
        s.elements.truncate(n);
        s.to_json()
    };
    let (mut app, d) = app_in("drawreload", &[("D.excalidraw", scene(3).as_bytes())]);
    app.open(Path::new("D.excalidraw"), false);
    assert_eq!(app.drawings[Path::new("D.excalidraw")].scene.elements.len(), 3);
    app.rescan();
    assert_eq!(app.drawings[Path::new("D.excalidraw")].scene.elements.len(), 3, "an unchanged file keeps the open drawing");
    crate::fsutil::write_atomic(&d.join("D.excalidraw"), scene(1).as_bytes()).unwrap();
    app.rescan();
    assert_eq!(app.drawings[Path::new("D.excalidraw")].scene.elements.len(), 1, "changed outside: reloaded");
    // with unsaved edits the drawing is left alone until it is saved (then it is a conflict, below)
    app.drawings.get_mut(Path::new("D.excalidraw")).unwrap().dirty = true;
    crate::fsutil::write_atomic(&d.join("D.excalidraw"), scene(2).as_bytes()).unwrap();
    app.rescan();
    assert_eq!(app.drawings[Path::new("D.excalidraw")].scene.elements.len(), 1);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: DATA-003
#[test]
fn an_edit_over_an_outside_change_becomes_a_conflict_copy() {
    let (mut app, d) = app_in("conflict", &[("N.md", b"base\n")]);
    app.open(Path::new("N.md"), false);
    type_text(&mut app, "N.md", "my edit\n");
    // another program writes the file before the autosave
    crate::fsutil::write_atomic(&d.join("N.md"), b"their edit\n").unwrap();
    app.autosave();
    // the other program's version stays at the path, mine is next to it, both are intact
    assert_eq!(read(&d, "N.md"), "their edit\n");
    let copies = names(&d, "N (conflict ");
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert!(copies[0].ends_with(").md"));
    assert_eq!(read(&d, &copies[0]), "my edit\n");
    // the buffer shows the other version, is clean, and the user is told with both names
    let doc = &app.notes[Path::new("N.md")];
    assert_eq!((doc.text.as_str(), doc.dirty), ("their edit\n", false));
    assert!(toasted(&app, &copies[0]) && toasted(&app, "“N.md”"), "{:?}", app.toasts);
    // the copy is a note in the vault like any other
    assert!(app.vault.as_ref().unwrap().file(Path::new(&copies[0])).is_some());
    // typing on goes to the file again, no second conflict
    type_text(&mut app, "N.md", "more\n");
    app.autosave();
    assert_eq!(read(&d, "N.md"), "more\n");
    assert_eq!(names(&d, "N (conflict ").len(), 1);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: DATA-003
#[test]
fn conflict_copies_never_replace_each_other() {
    let (mut app, d) = app_in("conflict-many", &[("N.md", b"base\n")]);
    app.open(Path::new("N.md"), false);
    for i in 0..4 {
        type_text(&mut app, "N.md", &format!("mine {i}\n"));
        crate::fsutil::write_atomic(&d.join("N.md"), format!("theirs {i}\n").as_bytes()).unwrap();
        app.autosave();
    }
    let copies = names(&d, "N (conflict ");
    assert_eq!(copies.len(), 4, "{copies:?}");
    let mut texts: Vec<String> = copies.iter().map(|c| read(&d, c)).collect();
    texts.sort();
    assert_eq!(texts, vec!["mine 0\n", "mine 1\n", "mine 2\n", "mine 3\n"]);
    assert_eq!(read(&d, "N.md"), "theirs 3\n");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: DATA-003
#[test]
fn a_touched_file_with_the_same_content_is_no_conflict() {
    let (mut app, d) = app_in("touched", &[("N.md", b"base\n")]);
    app.open(Path::new("N.md"), false);
    type_text(&mut app, "N.md", "mine\n");
    crate::fsutil::write_atomic(&d.join("N.md"), b"base\n").unwrap(); // a sync tool rewrote it
    app.autosave();
    assert_eq!(read(&d, "N.md"), "mine\n");
    assert!(names(&d, "N (conflict ").is_empty());
    // both sides made the same edit: nothing to keep apart either
    type_text(&mut app, "N.md", "same\n");
    crate::fsutil::write_atomic(&d.join("N.md"), b"same\n").unwrap();
    app.autosave();
    assert_eq!(read(&d, "N.md"), "same\n");
    assert!(names(&d, "N (conflict ").is_empty());
    assert!(!app.notes[Path::new("N.md")].dirty);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: DATA-003
#[test]
fn a_deleted_note_with_edits_is_written_again() {
    let (mut app, d) = app_in("deleted", &[("N.md", b"base\n")]);
    app.open(Path::new("N.md"), false);
    type_text(&mut app, "N.md", "mine\n");
    std::fs::remove_file(d.join("N.md")).unwrap();
    app.autosave();
    assert_eq!(read(&d, "N.md"), "mine\n");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: DATA-003
#[test]
fn a_drawing_with_edits_over_an_outside_change_becomes_a_conflict_copy() {
    let scene = |n: usize| {
        let mut s = crate::drawing::Scene::parse(crate::drawing::SAMPLE_SCENE).unwrap();
        s.elements.truncate(n);
        s.to_json()
    };
    let (mut app, d) = app_in("drawconflict", &[("D.excalidraw", scene(3).as_bytes())]);
    app.open(Path::new("D.excalidraw"), false);
    let doc = app.drawings.get_mut(Path::new("D.excalidraw")).unwrap();
    doc.scene.elements.truncate(2);
    doc.dirty = true;
    doc.last_edit = Instant::now() - Duration::from_secs(5);
    crate::fsutil::write_atomic(&d.join("D.excalidraw"), scene(1).as_bytes()).unwrap();
    app.autosave();
    assert_eq!(read(&d, "D.excalidraw"), scene(1));
    let copies = names(&d, "D (conflict ");
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert!(copies[0].ends_with(").excalidraw"), "{copies:?}");
    assert_eq!(crate::drawing::Scene::parse(&read(&d, &copies[0])).unwrap().elements.len(), 2);
    let doc = &app.drawings[Path::new("D.excalidraw")];
    assert_eq!((doc.scene.elements.len(), doc.dirty), (1, false));
    assert!(toasted(&app, &copies[0]));
    let _ = std::fs::remove_dir_all(&d);
}

/// Make a folder read-only (`on`) or writable again. Only on Unix does that close the folder: a
/// read-only folder on Windows still takes new files, so there this does nothing and the tests
/// that need a closed folder skip themselves with a note (`locks_do_not_hold`).
fn locked(dir: &Path, on: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(if on { 0o555 } else { 0o755 })).unwrap();
    }
    #[cfg(not(unix))]
    let _ = (dir, on);
}

/// Can this process write into a read-only folder (running as root, for example)?
fn locks_do_not_hold(d: &Path) -> bool {
    locked(d, true);
    let works = crate::fsutil::create_new(&d.join("probe.tmp"), b"x").is_ok();
    locked(d, false);
    let _ = std::fs::remove_file(d.join("probe.tmp"));
    if works {
        eprintln!("skipping: a read-only folder still takes files here (running as root, or not Unix)");
    }
    works
}

// spec: DATA-009
#[test]
fn a_failed_save_keeps_the_buffer_dirty_marks_the_tab_and_is_shown_once() {
    let (mut app, d) = app_in("failsave", &[("N.md", b"base\n"), ("D.excalidraw", crate::drawing::SAMPLE_SCENE.as_bytes())]);
    if locks_do_not_hold(&d) {
        return;
    }
    app.open(Path::new("N.md"), false);
    app.open(Path::new("D.excalidraw"), true);
    type_text(&mut app, "N.md", "precious\n");
    let doc = app.drawings.get_mut(Path::new("D.excalidraw")).unwrap();
    doc.scene.elements.truncate(1);
    doc.dirty = true;
    doc.last_edit = Instant::now() - Duration::from_secs(5);

    locked(&d, true);
    app.autosave();
    let doc = &app.notes[Path::new("N.md")];
    assert!(doc.dirty, "nothing is lost: the buffer stays dirty");
    assert_eq!(doc.text, "precious\n");
    assert!(doc.save_error.as_deref().unwrap_or("").contains("Could not save"), "{:?}", doc.save_error);
    assert!(app.drawings[Path::new("D.excalidraw")].dirty && app.drawings[Path::new("D.excalidraw")].save_error.is_some());
    assert!(toasted(&app, "Could not save “N.md”") && toasted(&app, "Could not save “D.excalidraw”"), "{:?}", app.toasts);
    assert_eq!(read(&d, "N.md"), "base\n");
    assert!(names(&d, ".shale-tmp").is_empty(), "no temporary file is left");

    // it retries, but does not show the same message again and again
    let shown = app.toasts.len();
    for _ in 0..3 {
        app.notes.get_mut(Path::new("N.md")).unwrap().last_edit = Instant::now() - Duration::from_secs(5);
        app.autosave();
    }
    assert_eq!(app.toasts.len(), shown, "{:?}", app.toasts);

    // when the problem is gone the next autosave saves, and the marks go away
    locked(&d, false);
    app.notes.get_mut(Path::new("N.md")).unwrap().last_edit = Instant::now() - Duration::from_secs(5);
    app.drawings.get_mut(Path::new("D.excalidraw")).unwrap().last_edit = Instant::now() - Duration::from_secs(5);
    app.autosave();
    assert_eq!(read(&d, "N.md"), "precious\n");
    let doc = &app.notes[Path::new("N.md")];
    assert!(!doc.dirty && doc.save_error.is_none());
    let doc = &app.drawings[Path::new("D.excalidraw")];
    assert!(!doc.dirty && doc.save_error.is_none());
    let _ = std::fs::remove_dir_all(&d);
}

// spec: DATA-009
#[test]
fn toggling_a_checkbox_that_cannot_be_saved_is_shown() {
    let (mut app, d) = app_in("failtoggle", &[("T.md", b"- [ ] one\n"), ("U.md", b"- [ ] two\n")]);
    if locks_do_not_hold(&d) {
        return;
    }
    app.open(Path::new("T.md"), false);
    locked(&d, true);
    app.toggle_task(Path::new("T.md"), 3);
    app.toggle_task(Path::new("U.md"), 3);
    locked(&d, false);
    assert!(app.notes[Path::new("T.md")].dirty, "kept in the buffer");
    assert!(app.notes[Path::new("T.md")].text.contains("[x]"));
    assert!(toasted(&app, "Could not save “T.md”") && toasted(&app, "Could not change “U.md”"), "{:?}", app.toasts);
    assert_eq!(read(&d, "U.md"), "- [ ] two\n");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: VAULT-016
#[test]
fn a_rename_reports_the_notes_it_could_not_update_and_updates_the_others() {
    let (mut app, d) = app_in("relocate", &[("Target.md", b"t\n"), ("A.md", b"see [[Target]]\n"), ("B.md", b"see [[Target]]\n"), ("C.md", b"see [[Target]]\n")]);
    // B changes on disk after the index was made: it is not touched
    crate::fsutil::write_atomic(&d.join("B.md"), b"see [[Target]] and my new text\n").unwrap();
    app.rename(Path::new("Target.md"), "Goal").unwrap();
    assert_eq!(read(&d, "A.md"), "see [[Goal]]\n");
    assert_eq!(read(&d, "C.md"), "see [[Goal]]\n");
    assert_eq!(read(&d, "B.md"), "see [[Target]] and my new text\n", "the other program's change is kept as it is");
    let v = app.vault.as_ref().unwrap();
    assert_eq!(v.failed_rewrites.len(), 1);
    assert_eq!(v.failed_rewrites[0].0, PathBuf::from("B.md"));
    let t = app.toasts.iter().map(|(t, _)| t.clone()).find(|t| t.contains("could not be updated")).expect("a message");
    assert!(t.contains("“B.md”") && t.contains("“A.md”") && t.contains("“C.md”"), "which were and were not: {t}");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: VAULT-016
#[test]
fn a_rename_where_a_write_fails_halfway_says_which_notes_were_updated() {
    let (mut app, d) = app_in("relocate-fail", &[("Target.md", b"t\n"), ("A.md", b"see [[Target]]\n"), ("sub/B.md", b"see [[Target]]\n"), ("C.md", b"see [[Target]]\n")]);
    if locks_do_not_hold(&d) {
        return;
    }
    locked(&d.join("sub"), true);
    let r = app.rename(Path::new("Target.md"), "Goal");
    locked(&d.join("sub"), false);
    r.unwrap();
    assert_eq!(read(&d, "A.md"), "see [[Goal]]\n");
    assert_eq!(read(&d, "C.md"), "see [[Goal]]\n");
    assert_eq!(read(&d, "sub/B.md"), "see [[Target]]\n", "the original is untouched");
    assert!(d.join("Goal.md").exists());
    let v = app.vault.as_ref().unwrap();
    assert_eq!(v.failed_rewrites.iter().map(|f| f.0.clone()).collect::<Vec<_>>(), vec![PathBuf::from("sub/B.md")]);
    let t = app.toasts.iter().map(|(t, _)| t.clone()).find(|t| t.contains("could not be updated")).expect("a message");
    assert!(t.contains("sub/B.md") && t.contains("A.md") && t.contains("C.md"), "{t}");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: DATA-009
#[test]
fn a_folder_with_unsaved_edits_that_cannot_be_saved_is_not_moved_to_the_trash() {
    let (mut app, d) = app_in("trash-unsaved", &[("x/N.md", b"base\n"), ("x/M.md", b"m\n")]);
    if locks_do_not_hold(&d) {
        return;
    }
    app.open(Path::new("x/N.md"), false);
    type_text(&mut app, "x/N.md", "precious\n");
    locked(&d.join("x"), true);
    app.trash(Path::new("x"));
    locked(&d.join("x"), false);
    assert!(d.join("x/N.md").exists(), "the folder stays");
    assert!(!d.join(".trash").exists());
    let doc = &app.notes[Path::new("x/N.md")];
    assert!(doc.dirty && doc.text == "precious\n", "the unsaved text is still there");
    assert!(toasted(&app, "not moved to the trash"), "{:?}", app.toasts);
    // once it can be saved, the trash works and the edit is in the trashed file
    app.trash(Path::new("x"));
    assert!(!d.join("x").exists());
    let day = crate::mail::store::today_string();
    assert!(names(&d.join(".trash").join(&day), "x").len() == 1);
    assert_eq!(read(&d, &format!(".trash/{day}/x/N.md")), "precious\n");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: DATA-011
#[test]
fn text_that_cannot_be_saved_at_exit_is_put_in_the_recovery_folder_and_announced_next_start() {
    let (mut app, d) = app_in("exitrescue", &[("Sub/N.md", b"base\n")]);
    if locks_do_not_hold(&d.join("Sub")) {
        return;
    }
    let rec = d.join("recovery-root");
    app.recovery.root = rec.clone();
    app.open(Path::new("Sub/N.md"), false);
    type_text(&mut app, "Sub/N.md", "precious unsaved words\n");
    locked(&d.join("Sub"), true);
    eframe::App::on_exit(&mut app);
    locked(&d.join("Sub"), false);
    assert_eq!(read(&d, "Sub/N.md"), "base\n", "the note on disk is untouched");
    let day = crate::mail::store::today_string();
    let saved = rec.join(&day).join("Sub/N.md");
    assert_eq!(std::fs::read_to_string(&saved).unwrap(), "precious unsaved words\n");

    // the next start announces it, once dismissed it stays quiet
    let mut next = ShaleApp::build(Options { screenshot: None, vault: Some(d.clone()), view: None, keys: None }, false);
    next.recovery.root = rec.clone();
    next.recovery.notice = crate::recovery::unseen(&rec);
    assert_eq!(next.recovery.notice, vec![rec.join(&day)]);
    crate::recovery::dismiss(&next.recovery.notice);
    assert!(crate::recovery::unseen(&rec).is_empty());

    // a clean exit rescues nothing
    let (mut ok, d2) = app_in("exitclean", &[("N.md", b"x\n")]);
    ok.recovery.root = d2.join("rec");
    ok.open(Path::new("N.md"), false);
    eframe::App::on_exit(&mut ok);
    assert!(!d2.join("rec").exists());
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&d2);
}

// spec: VAULT-020
#[test]
fn restore_last_deleted_puts_the_item_back_and_refuses_when_the_place_is_taken() {
    let (mut app, d) = app_in("restore-cmd", &[("x/N.md", b"base\n"), ("y/N.md", b"other\n")]);
    assert!(!app.can_restore_trashed());
    app.trash(Path::new("x/N.md"));
    app.trash(Path::new("y/N.md"));
    assert!(app.can_restore_trashed());
    // newest first
    app.actions.push(Action::RestoreTrashed);
    app.process_actions();
    assert_eq!(read(&d, "y/N.md"), "other\n");
    assert!(toasted(&app, "Restored"), "{:?}", app.toasts);
    // something else took the place of the next one: it stays in the trash
    crate::fsutil::write_atomic(&d.join("x/N.md"), b"new\n").unwrap();
    app.restore_trashed();
    assert!(toasted(&app, "Could not restore"), "{:?}", app.toasts);
    assert_eq!(read(&d, "x/N.md"), "new\n");
    assert!(app.can_restore_trashed());
    let _ = std::fs::remove_dir_all(&d);
}
