//! The history in the app: snapshots on save, idle and close, restore, deleted notes, rename and
//! the dialog (SYNC-032…SYNC-046), against real git in temporary folders.

use super::*;
use crate::app::Options as AppOptions;
use std::ffi::OsString;
use std::time::Instant;

fn hermetic() -> Git {
    let env = [("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_CONFIG_NOSYSTEM", "1"), ("GIT_CONFIG_COUNT", "1"), ("GIT_CONFIG_KEY_0", "init.defaultBranch"), ("GIT_CONFIG_VALUE_0", "main")];
    let mut git = Git { program: "git".into(), env: env.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))).collect() };
    crate::gitsync::hide_lfs(&mut git);
    git
}

pub(super) fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    crate::fsutil::write_atomic(&p, text.as_bytes()).unwrap();
}

/// A window on a vault, with history on and a clock that tests set.
pub(super) struct T {
    pub(super) app: ShaleApp,
    pub(super) ctx: egui::Context,
    root: PathBuf,
    pub(super) vault: PathBuf,
}

pub(super) fn setup(tag: &str, git_init: bool) -> T {
    let root = std::env::temp_dir().join(format!("shale-app-history-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let vault = root.join("vault");
    std::fs::create_dir_all(&vault).unwrap();
    if git_init {
        assert!(std::process::Command::new("git").arg("-C").arg(&vault).args(["init", "--quiet"]).envs(hermetic().env.iter().cloned()).status().unwrap().success());
    }
    write(&vault, "Note.md", "one\n");
    T { app: ShaleApp::build(AppOptions { screenshot: None, vault: Some(vault.clone()), view: None, keys: None }, false), ctx: egui::Context::default(), root, vault }
}

impl T {
    pub(super) fn start(&mut self) {
        self.app.sync.git = hermetic();
        self.app.refresh_lfs();
        self.app.history.on = true;
        self.app.history.fake_now = Some(1_000);
        self.settle();
    }

    /// Frames until no snapshot job and no read is left (not the saves that wait for their time).
    pub(super) fn settle(&mut self) {
        let t = Instant::now();
        self.app.history_tick(&self.ctx);
        while !self.app.history_quiet() {
            assert!(t.elapsed() < Duration::from_secs(30), "the history did not settle");
            std::thread::sleep(Duration::from_millis(5));
            self.app.history_tick(&self.ctx);
        }
        self.app.history_tick(&self.ctx);
        while !self.app.history_quiet() {
            assert!(t.elapsed() < Duration::from_secs(30), "the history did not settle");
            std::thread::sleep(Duration::from_millis(5));
            self.app.history_tick(&self.ctx);
        }
    }

    /// The versions of a note as the panel reads them.
    pub(super) fn versions(&mut self, rel: &str) -> Vec<crate::gitsync::history::Version> {
        self.app.history.panel.stale = true;
        self.app.history.panel.want = Some(PathBuf::from(rel));
        self.settle();
        self.app.history.panel.versions.clone()
    }

    pub(super) fn at(&mut self, secs: u64) {
        self.app.history.fake_now = Some(secs);
    }

    pub(super) fn repo(&self) -> Repo {
        history::locate(&hermetic(), &self.vault, &self.app.usage.places.history).unwrap()
    }

    pub(super) fn messages(&self) -> Vec<crate::gitsync::history::Meta> {
        let o = self.repo().run_for_test(&["log", "-z", "--format=%B"]);
        o.split('\0').filter(|m| !m.trim().is_empty()).map(history::parse_message).collect()
    }

    pub(super) fn reasons(&self) -> Vec<Option<Reason>> {
        self.messages().iter().map(|m| m.reason).collect()
    }

    pub(super) fn type_into(&mut self, rel: &str, text: &str) {
        let doc = self.app.notes.get_mut(Path::new(rel)).unwrap();
        doc.text = text.into();
        doc.dirty = true;
        self.app.save_note(Path::new(rel));
    }

    pub(super) fn toasted(&self, part: &str) -> bool {
        self.app.toasts.iter().any(|(t, _)| t.contains(part))
    }
}

impl Drop for T {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

// spec: SYNC-033, SYNC-035, SYNC-044, SYNC-032
#[test]
fn saves_make_snapshots_that_coalesce_and_idle_and_close_make_one() {
    let mut t = setup("triggers", false);
    t.start();
    assert_eq!(t.reasons(), [Some(Reason::Initial)], "a new history starts with the vault as it is");
    t.app.open(Path::new("Note.md"), false);

    // the first save is snapshotted at once
    t.type_into("Note.md", "two\n");
    t.settle();
    assert_eq!(t.reasons(), [Some(Reason::Save), Some(Reason::Initial)]);
    let msg = &t.messages()[0];
    assert_eq!(msg.paths, ["Note.md"]);
    assert!(msg.device.is_some());

    // saves within the window make no snapshot while the buffer is being edited
    t.at(1_030);
    t.type_into("Note.md", "three\n");
    t.app.notes.get_mut(Path::new("Note.md")).unwrap().dirty = true; // typing goes on
    t.settle();
    assert_eq!(t.messages().len(), 2);
    // idle: no unsaved change and 60 s since the save
    t.app.notes.get_mut(Path::new("Note.md")).unwrap().dirty = false;
    t.at(1_080);
    t.settle();
    assert_eq!(t.messages().len(), 2, "not yet idle");
    t.at(1_091);
    t.settle();
    assert_eq!(t.reasons(), [Some(Reason::Idle), Some(Reason::Save), Some(Reason::Initial)]);

    // closing the tab
    t.at(1_100);
    t.type_into("Note.md", "four\n");
    t.app.close_tab(0);
    t.settle();
    assert_eq!(t.reasons()[0], Some(Reason::Close));
    assert_eq!(t.repo().run_for_test(&["show", "HEAD:Note.md"]), "four\n");
    // nothing but the ID file was written into the vault
    let mut names: Vec<String> = std::fs::read_dir(&t.vault).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, [".shale", "Note.md"]);
}

// spec: SYNC-032
#[test]
fn history_off_makes_no_snapshot_and_says_why() {
    let mut t = setup("off", false);
    t.start();
    assert_eq!(t.messages().len(), 1);
    assert_eq!(t.app.history_off_reason(), None);
    t.app.cfg.history.enabled = false;
    assert!(t.app.history_off_reason().unwrap().contains("switched off"));
    t.app.open(Path::new("Note.md"), false);
    t.type_into("Note.md", "two\n");
    t.settle();
    assert_eq!(t.messages().len(), 1, "no snapshot while it is off");
    t.app.cfg.history.enabled = true;
    t.app.sync.git = Git { program: "/nonexistent/git".into(), env: vec![] };
    t.app.sync.forget_git_version();
    assert!(!t.app.history_active());
    assert!(t.app.history_off_reason().unwrap().contains("git is not installed"));
}

// spec: SYNC-035
#[test]
fn the_sync_does_not_start_while_a_snapshot_runs() {
    let mut t = setup("exclusive", true);
    t.start();
    t.app.open(Path::new("Note.md"), false);
    t.type_into("Note.md", "two\n");
    t.app.history_tick(&t.ctx); // starts the snapshot job
    assert!(t.app.history.busy());
    t.app.start_sync();
    assert!(!t.app.sync.running(), "no sync while a snapshot runs");
    t.settle();
    assert!(!t.app.history.busy());
    t.app.start_sync();
    assert!(t.app.sync.running(), "the sync starts when the snapshot is done");
    // and no snapshot while the sync runs
    t.type_into("Note.md", "three\n");
    t.at(2_000);
    t.app.history_tick(&t.ctx);
    assert!(!t.app.history.busy() || !t.app.sync.running());
    let end = Instant::now();
    while t.app.sync.running() {
        assert!(end.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(10));
        t.app.sync_tick(&t.ctx);
    }
    t.settle();
    assert!(t.reasons().contains(&Some(Reason::Sync)) || t.reasons().contains(&Some(Reason::Save)));
}

// spec: SYNC-038, SYNC-043
#[test]
fn restoring_a_version_into_an_open_note_is_one_undoable_change_with_snapshots_around_it() {
    let mut t = setup("restore", false);
    t.start();
    t.app.open(Path::new("Note.md"), false);
    t.type_into("Note.md", "two\n");
    t.settle();
    t.at(2_000);
    t.type_into("Note.md", "three\n");
    t.settle();
    let before: Vec<String> = t.repo().run_for_test(&["rev-list", "HEAD"]).lines().map(str::to_string).collect();
    assert_eq!(before.len(), 3);

    // the versions of the note
    let versions = t.versions("Note.md");
    assert_eq!(versions.len(), 3, "{versions:?}");
    // type something unsaved, then restore the first version ("one")
    let doc = t.app.notes.get_mut(Path::new("Note.md")).unwrap();
    doc.text = "three, and typed\n".into();
    doc.dirty = true;
    let first = versions.last().unwrap().clone();
    t.app.history_restore_version(Path::new("Note.md"), &first);
    t.settle();

    // the note shows the old text, saved, and the editor's undo goes back to what was there
    assert_eq!(t.app.notes[Path::new("Note.md")].text, "one\n");
    assert!(!t.app.notes[Path::new("Note.md")].dirty);
    assert_eq!(crate::fsutil::read_to_string(t.vault.join("Note.md")).unwrap(), "one\n");
    let mut text = t.app.notes[Path::new("Note.md")].text.clone();
    assert!(t.app.undo.of(Path::new("Note.md")).undo(&mut text).is_some());
    assert_eq!(text, "three, and typed\n", "one undo step brings back the text before the restore");
    // history: the pre-restore snapshot holds the saved text, the restore snapshot names the version
    let after: Vec<String> = t.repo().run_for_test(&["rev-list", "HEAD"]).lines().map(str::to_string).collect();
    assert_eq!(after.len(), 5, "{after:?}");
    assert_eq!(&after[2..], &before[..], "earlier commits are untouched");
    let msgs = t.messages();
    assert_eq!(msgs[0].reason, Some(Reason::Restore));
    assert_eq!(msgs[0].restored.as_deref(), Some(first.commit.as_str()));
    assert_eq!(msgs[1].reason, Some(Reason::Save));
    assert_eq!(t.repo().run_for_test(&["show", &format!("{}:Note.md", after[1])]), "three, and typed\n");
    assert_eq!(t.repo().run_for_test(&["show", "HEAD:Note.md"]), "one\n");
    assert!(t.toasted("Restored “Note.md”"), "{:?}", t.app.toasts);
}

// spec: SYNC-038
#[test]
fn restoring_a_version_of_a_note_that_is_not_open_writes_it_like_a_save() {
    let mut t = setup("restore-closed", false);
    t.start();
    // the window opened the note: close its tab, so that it is not open
    t.app.close_tab(0);
    t.app.notes.remove(Path::new("Note.md"));
    write(&t.vault, "Note.md", "two\n");
    t.app.history.note_saved(Path::new("Note.md"));
    t.settle();
    let old = t.versions("Note.md").last().unwrap().clone();
    assert_eq!(old.meta.reason, Some(Reason::Initial));
    // the text on disk now has no snapshot yet; the restore makes one first
    write(&t.vault, "Note.md", "three\n");
    t.app.history_restore_version(Path::new("Note.md"), &old);
    t.settle();
    assert_eq!(crate::fsutil::read_to_string(t.vault.join("Note.md")).unwrap(), "one\n");
    assert!(!t.app.notes.contains_key(Path::new("Note.md")), "it was not opened by the restore");
    assert_eq!(t.reasons()[..2], [Some(Reason::Restore), Some(Reason::Save)]);
    assert_eq!(t.repo().run_for_test(&["show", "HEAD~1:Note.md"]), "three\n");
    assert_eq!(t.messages()[0].restored.as_deref(), Some(old.commit.as_str()));
}

// spec: SYNC-039
#[test]
fn a_version_is_shown_read_only_and_its_text_is_copied() {
    let mut t = setup("view", false);
    t.start();
    t.app.open(Path::new("Note.md"), false);
    t.type_into("Note.md", "two\n");
    t.settle();
    let v = t.versions("Note.md").last().unwrap().clone();
    t.app.history_view_version(&v);
    t.settle();
    let (title, text) = t.app.history.changes.alone.clone().unwrap();
    assert_eq!(text, "one\n");
    assert!(title.starts_with("Note.md as of "), "{title}");
    assert_eq!(t.app.notes[Path::new("Note.md")].text, "two\n", "viewing changes nothing");
    // copy: the text goes to the clipboard of the window
    t.app.ctx = Some(t.ctx.clone());
    t.app.history_copy_version(&v);
    t.settle();
    assert!(t.toasted("Copied the text"), "{:?}", t.app.toasts);
    let copied = t.ctx.output(|o| o.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(s) if s == "one\n")));
    assert!(copied, "the text of the version is copied");
}

// spec: SYNC-040, SYNC-046
#[test]
fn a_trashed_note_is_listed_as_deleted_with_its_last_text_and_comes_back_at_its_path_or_a_free_name() {
    let mut t = setup("deleted", false);
    t.start();
    t.app.open(Path::new("Note.md"), false);
    // saved a moment ago, not snapshotted: the trash makes it safe first
    t.at(1_010);
    t.type_into("Note.md", "last text\n");
    t.app.notes.get_mut(Path::new("Note.md")).unwrap().dirty = false;
    t.app.trash(Path::new("Note.md"));
    t.settle();
    assert!(!t.vault.join("Note.md").exists());
    assert_eq!(t.reasons()[0], Some(Reason::Delete));

    t.app.open_history_deleted();
    t.settle();
    let list = t.app.history.deleted.list.clone();
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0].path, Path::new("Note.md"));
    t.app.history_restore_deleted(&list[0]);
    t.settle();
    assert_eq!(crate::fsutil::read_to_string(t.vault.join("Note.md")).unwrap(), "last text\n");
    assert_eq!(t.reasons()[0], Some(Reason::Restore));
    assert_eq!(t.messages()[0].restored.as_deref(), Some(list[0].commit.as_str()));

    // the path is taken now: a free name
    std::fs::remove_file(t.vault.join("Note.md")).unwrap();
    t.app.vault.as_mut().unwrap().rescan();
    t.app.history_restore_deleted(&list[0]);
    t.settle();
    write(&t.vault, "Note.md", "new note\n");
    t.app.history_restore_deleted(&list[0]);
    t.settle();
    assert_eq!(crate::fsutil::read_to_string(t.vault.join("Note (restored).md")).unwrap(), "last text\n");
    assert_eq!(crate::fsutil::read_to_string(t.vault.join("Note.md")).unwrap(), "new note\n");
}

// spec: SYNC-041
#[test]
fn a_rename_makes_a_rename_snapshot_and_the_versions_follow_it() {
    let mut t = setup("rename", false);
    t.start();
    t.app.open(Path::new("Note.md"), false);
    t.type_into("Note.md", "two\n");
    t.settle();
    t.app.rename(Path::new("Note.md"), "Renamed").unwrap();
    t.settle();
    assert_eq!(t.reasons()[0], Some(Reason::Rename));
    assert_eq!(t.messages()[0].paths, ["Note.md", "Renamed.md"]);
    let v = &t.versions("Renamed.md");
    assert_eq!(v.iter().map(|v| v.path.to_string_lossy().into_owned()).collect::<Vec<_>>(), ["Renamed.md", "Note.md", "Note.md"]);
}

// spec: SYNC-033
#[test]
fn closing_the_window_snapshots_what_is_waiting() {
    let mut t = setup("exit", false);
    t.start();
    t.app.open(Path::new("Note.md"), false);
    t.type_into("Note.md", "two\n");
    t.settle();
    t.at(1_030);
    t.type_into("Note.md", "three\n");
    t.settle();
    assert_eq!(t.messages().len(), 2, "within the window: waiting");
    t.app.history_flush_now();
    assert_eq!(t.reasons()[0], Some(Reason::Close));
    assert_eq!(t.repo().run_for_test(&["show", "HEAD:Note.md"]), "three\n");
}

// spec: SYNC-032, CFG-066, SYNC-045
#[test]
fn the_settings_switch_history_and_set_its_times_and_open_the_deleted_list() {
    let mut t = setup("settings", false);
    t.start();
    let mut time = 0.0;
    let mut asks = crate::settings::Asks::default();
    let mut frame = |t: &mut T, asks: &mut crate::settings::Asks, events: Vec<egui::Event>| -> bool {
        time += 0.05;
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0))), time: Some(time), events, ..Default::default() };
        let ctx = t.ctx.clone();
        let mut changed = false;
        let mut out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| changed = t.app.history_settings_ui(ui, asks));
        });
        out.textures_delta.clear();
        changed
    };
    frame(&mut t, &mut asks, vec![]);
    let click = |t: &mut T, asks: &mut crate::settings::Asks, frame: &mut dyn FnMut(&mut T, &mut crate::settings::Asks, Vec<egui::Event>) -> bool, key: &str| -> bool {
        let p = t.app.sync.rects.iter().find(|r| r.0 == key).unwrap_or_else(|| panic!("no {key}")).1.center();
        let b = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        let mut changed = frame(t, asks, vec![egui::Event::PointerMoved(p)]);
        changed |= frame(t, asks, vec![b(true)]);
        changed |= frame(t, asks, vec![b(false)]);
        changed | frame(t, asks, vec![])
    };
    assert!(t.app.cfg.history.enabled);
    assert!(click(&mut t, &mut asks, &mut frame, "history-enabled"));
    assert!(!t.app.cfg.history.enabled);
    assert!(click(&mut t, &mut asks, &mut frame, "history-enabled"));
    assert!(t.app.cfg.history.enabled);
    assert!(!asks.history_deleted);
    click(&mut t, &mut asks, &mut frame, "history-deleted");
    assert!(asks.history_deleted);
    for key in ["history-minutes", "history-idle"] {
        assert!(t.app.sync.rects.iter().any(|r| r.0 == key), "{key}");
    }
}
