//! The settings file as a tab (CFG-011 to CFG-014), driven through the whole window (headless).
//! The settings file of each test is its own (`TestConfig`), never the real one.

use crate::app::{Options, SettingsTab, ShaleApp, View};
use crate::config::testing::TestConfig;
use crate::config::Config;
use crate::ui_test_support::Headless;
use eframe::egui::PointerButton;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A window on the sample vault whose settings are saved to a file of this test.
struct W {
    app: ShaleApp,
    hl: Headless,
    dir: PathBuf,
    t: TestConfig,
}

impl W {
    fn new(tag: &str) -> W {
        let t = TestConfig::new(tag);
        let dir = std::env::temp_dir().join(format!("shale-settings-file-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::vault::write_sample_vault(&dir).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.persist_config = true;
        app.save_config();
        app.settings_file_baseline();
        let mut w = W { app, hl: Headless::new(), dir, t };
        w.hl.frames(&mut w.app, 3);
        w.wait_for_tag_learner();
        w
    }

    /// The window learns the tag words of the vault in the background and keeps them in
    /// `.shale/tag-words.json` when that is done, on a frame of its own. Wait for it, so that
    /// the file does not appear in the middle of a test that looks at the vault's files.
    fn wait_for_tag_learner(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.app.tag_learner.running() {
            assert!(Instant::now() < deadline, "the tag words were not learned in 30 s");
            std::thread::sleep(Duration::from_millis(5));
            self.hl.frames(&mut self.app, 1);
        }
    }

    fn file(&self) -> PathBuf {
        self.t.file()
    }

    fn disk(&self) -> String {
        std::fs::read_to_string(self.file()).unwrap()
    }

    /// What another program does: replace the file.
    fn write_outside(&self, text: &str) {
        crate::config::write_private(&self.file(), text.as_bytes()).unwrap();
    }

    /// The files of the vault folder, by their path in it.
    fn vault_files(&self) -> Vec<String> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
                if p.is_dir() {
                    walk(&p, root, out);
                }
            }
        }
        let mut out = vec![];
        walk(&self.dir, &self.dir, &mut out);
        out.sort();
        out
    }

    /// What the window said about the settings file (the keyring notices of other tests are not ours).
    fn toasts(&self) -> Vec<String> {
        self.app.toasts.iter().map(|(m, _)| m.clone()).filter(|m| m.starts_with("The settings") || m.contains("settings file")).collect()
    }

    /// Let a second pass and look at the file.
    fn look(&mut self) {
        self.app.sf.checked = Instant::now() - Duration::from_secs(2);
        self.hl.frames(&mut self.app, 2);
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// spec: CFG-011
#[test]
fn cfg_011_the_button_opens_the_settings_file_in_a_tab_without_making_a_note() {
    let mut w = W::new("open");
    let (files, notes) = (w.vault_files(), w.app.notes.len());
    w.app.open_settings(SettingsTab::Vault);
    w.hl.frames(&mut w.app, 3);
    let r = w.hl.find_label(&mut w.app, "Open settings file");
    w.hl.click(&mut w.app, r.center(), PointerButton::Primary);
    assert!(w.app.modal.is_none(), "the dialog closes");
    assert_eq!(w.app.tabbar.tabs[w.app.tabbar.active].view, View::SettingsFile);
    assert_eq!(w.app.sf.text, w.disk(), "the tab shows the file");
    assert!(w.app.sf.text.starts_with("# Shale settings") && w.app.sf.text.contains("toolbar_size = 110"));
    // it is drawn as text, in a tab called for what it is
    w.hl.frames(&mut w.app, 3);
    assert!(w.hl.text.iter().any(|(t, _)| t.contains("# The size of the toolbar's buttons in percent")), "the text is on screen");
    assert!(w.hl.text.iter().any(|(t, _)| t == "Settings file"), "the tab has a title");
    // no note, no file in the vault, nothing in the lists of the vault
    assert_eq!(w.vault_files(), files, "nothing was created in the vault");
    assert_eq!(w.app.notes.len(), notes);
    assert!(w.app.vault.as_ref().unwrap().file(Path::new("config.toml")).is_none());
    w.app.remember_open_files();
    assert!(w.app.vdata.state.tabs.iter().all(|t| t.file.is_none() && t.feed.is_none()), "the tab is not remembered with the vault's tabs");
    assert!(!w.app.cfg.recent.iter().any(|p| p.ends_with("config.toml")));
    // a second time shows the same tab
    let n = w.app.tabbar.tabs.len();
    w.app.open_settings_file();
    assert_eq!(w.app.tabbar.tabs.len(), n);
    // without a settings file (nothing is saved in this mode) it says so
    let mut w2 = W::new("open-none");
    w2.app.persist_config = false;
    w2.app.open_settings_file();
    assert!(w2.toasts().iter().any(|m| m.contains("no settings file")) && !w2.app.tabbar.tabs.iter().any(|t| t.view == View::SettingsFile));
}

// spec: CFG-012, CFG-002
#[test]
fn cfg_012_saving_the_tab_writes_the_file_and_applies_the_setting() {
    let mut w = W::new("save");
    w.app.open_settings_file();
    assert!(!w.app.cfg.vim_mode);
    w.app.sf.text = w.app.sf.text.replace("toolbar_size = 110", "toolbar_size = 19").replace("vim_mode = false", "vim_mode = true").replace("picture_days = 30", "picture_days = \"none\"");
    w.app.sf.dirty = true;
    assert!(w.app.save_settings_file(true));
    assert!(!w.app.sf.dirty && w.app.sf.error.is_none());
    assert_eq!((w.app.cfg.toolbar_size, w.app.cfg.vim_mode, w.app.cfg.caches.picture_days), (19, true, None), "used at once");
    assert!(w.disk().contains("toolbar_size = 19"), "the file is what the tab held");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(w.file()).unwrap().permissions().mode() & 0o777, 0o600);
    }
    // it is not taken for a change by another program
    w.look();
    assert!(!w.app.sf.disk_changed && w.toasts().is_empty(), "{:?}", w.toasts());
    // the next change of a setting writes the file again, with its comments, and the tab follows
    w.app.cfg.dark_canvas = false;
    w.app.save_config();
    assert_eq!(w.app.sf.text, w.disk());
    assert!(w.app.sf.text.contains("# true: drawings are drawn on a dark canvas.\ndark_canvas = false"));
    // a tab that rests is saved by itself
    w.app.sf.text = w.app.sf.text.replace("toolbar_size = 19", "toolbar_size = 13");
    w.app.sf.dirty = true;
    w.app.sf.last_edit = Instant::now() - Duration::from_secs(2);
    w.hl.frames(&mut w.app, 2);
    assert_eq!(w.app.cfg.toolbar_size, 13);
    assert!(w.disk().contains("toolbar_size = 13") && !w.app.sf.dirty);
}

// spec: CFG-013
#[test]
fn cfg_013_a_change_made_by_another_program_is_picked_up_and_shale_s_own_writes_are_not() {
    let mut w = W::new("outside");
    w.app.open_settings_file();
    // Shale's own write
    w.app.cfg.media_volume = 0.3;
    w.app.save_config();
    w.look();
    assert!(w.toasts().is_empty() && !w.app.sf.disk_changed, "{:?}", w.toasts());
    assert!(w.app.sf.text.contains("media_volume = 0.3"), "the tab shows what Shale wrote");
    // another program
    let edited = w.disk().replace("toolbar_size = 110", "toolbar_size = 21");
    w.write_outside(&edited);
    w.look();
    assert_eq!(w.app.cfg.toolbar_size, 21, "used at once");
    assert_eq!(w.app.sf.text, edited, "the tab shows it");
    assert!(!w.app.sf.dirty && w.app.sf.error.is_none());
    assert_eq!(w.app.cfg.media_volume, 0.3);
    // the look is once a second
    let edited = edited.replace("toolbar_size = 21", "toolbar_size = 22");
    w.write_outside(&edited);
    w.app.sf.checked = Instant::now();
    w.hl.frames(&mut w.app, 2);
    assert_eq!(w.app.cfg.toolbar_size, 21, "not yet");
    w.look();
    assert_eq!(w.app.cfg.toolbar_size, 22);
    // the same text again (a program that only touches the file) changes nothing
    w.write_outside(&edited);
    w.look();
    assert!(w.toasts().is_empty());
}

// spec: CFG-013
#[test]
fn cfg_013_a_change_from_outside_does_not_replace_unsaved_text_of_the_tab() {
    let mut w = W::new("outside-dirty");
    w.app.open_settings_file();
    let mine = w.app.sf.text.replace("toolbar_size = 110", "toolbar_size = 11");
    w.app.sf.text = mine.clone();
    w.app.sf.dirty = true;
    w.write_outside(&w.disk().replace("show_left = true", "show_left = false"));
    w.app.check_settings_file(true);
    assert!(!w.app.cfg.show_left, "the settings of the file are used");
    assert_eq!(w.app.sf.text, mine, "the text of the tab stays");
    assert!(w.app.sf.dirty && w.app.sf.disk_changed);
    assert!(w.toasts().iter().any(|m| m.contains("changed by another program")));
    // it is not saved over the file by itself
    w.app.sf.last_edit = Instant::now() - Duration::from_secs(2);
    w.hl.frames(&mut w.app, 2);
    assert!(w.disk().contains("show_left = false") && w.app.sf.dirty);
    // loading the file from disk gives up the text of the tab
    let r = w.hl.find_label(&mut w.app, "Load the file from disk");
    w.hl.click(&mut w.app, r.center(), PointerButton::Primary);
    assert_eq!(w.app.sf.text, w.disk());
    assert!(!w.app.sf.dirty && !w.app.sf.disk_changed);
}

// spec: CFG-014
#[test]
fn cfg_014_text_with_an_error_is_not_saved_and_the_file_and_settings_stay() {
    let mut w = W::new("error");
    w.app.open_settings_file();
    let before = w.disk();
    let broken = w.app.sf.text.replace("toolbar_size = 110", "toolbar_size = [");
    w.app.sf.text = broken.clone();
    w.app.sf.dirty = true;
    w.app.sf.error = Config::parse_text(&broken).err();
    assert!(!w.app.save_settings_file(true));
    assert_eq!(w.disk(), before, "the file is as it was");
    assert_eq!(w.app.cfg.toolbar_size, 110, "the settings in use are as they were");
    assert_eq!(w.app.sf.text, broken, "the text stays in the tab");
    assert!(w.app.sf.dirty && w.app.sf.error.is_some());
    assert!(w.toasts().iter().any(|m| m.contains("not saved")), "{:?}", w.toasts());
    // a value of the wrong kind is an error too
    w.app.sf.text = before.replace("toolbar_size = 110", "toolbar_size = \"big\"");
    assert!(!w.app.save_settings_file(true) && w.disk() == before);
    // it does not save by itself, and the view says what is wrong
    w.app.sf.error = Config::parse_text(&w.app.sf.text).err();
    w.app.sf.last_edit = Instant::now() - Duration::from_secs(2);
    w.hl.frames(&mut w.app, 2);
    assert_eq!(w.disk(), before);
    assert!(w.hl.text.iter().any(|(t, _)| t.contains("is not valid settings")), "{:?}", w.hl.text.iter().map(|x| &x.0).collect::<Vec<_>>());
    // fixed, it is saved
    w.app.sf.text = before.replace("toolbar_size = 110", "toolbar_size = 16");
    w.app.sf.error = None;
    assert!(w.app.save_settings_file(true));
    assert_eq!(w.app.cfg.toolbar_size, 16);
}

// spec: CFG-014
#[test]
fn cfg_014_a_file_left_broken_from_outside_is_not_written_over_until_it_is_valid() {
    let mut w = W::new("outside-error");
    w.app.open_settings_file();
    let good = w.disk();
    let broken = good.replace("toolbar_size = 110", "toolbar_size = [");
    w.write_outside(&broken);
    w.look();
    assert!(w.toasts().iter().any(|m| m.contains("has an error")), "{:?}", w.toasts());
    assert_eq!(w.app.cfg.toolbar_size, 110, "the settings in use stay");
    assert!(w.app.sf.error.is_some() && w.app.sf.text == broken, "the tab shows it");
    // Shale does not write over it, and does not set it aside either
    w.app.cfg.dark_canvas = false;
    w.app.save_config();
    assert_eq!(w.disk(), broken);
    assert!(std::fs::read_dir(w.file().parent().unwrap()).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains("broken")));
    // one message for one problem
    let n = w.toasts().len();
    w.write_outside(&format!("{broken}\n# still broken\n"));
    w.look();
    assert_eq!(w.toasts().len(), n);
    // valid again: used, and written again
    w.write_outside(&good.replace("toolbar_size = 110", "toolbar_size = 18"));
    w.look();
    assert_eq!((w.app.cfg.toolbar_size, w.app.cfg.read_only), (18, false));
    assert!(w.app.sf.error.is_none());
    w.app.cfg.dark_canvas = false;
    w.app.save_config();
    assert!(w.disk().contains("dark_canvas = false"));
}

// spec: CFG-015
#[test]
fn cfg_015_unsaved_settings_text_goes_to_the_recovery_folder_at_exit() {
    let day = crate::mail::store::today_string();
    // text with an error: rescued, the file stays
    let mut w = W::new("exit-broken");
    let rec = w.dir.join("recovery-root");
    w.app.recovery.root = rec.clone();
    w.app.open_settings_file();
    let before = w.disk();
    let broken = before.replace("toolbar_size = 110", "toolbar_size = [");
    w.app.sf.text = broken.clone();
    w.app.sf.dirty = true;
    w.app.sf.error = Config::parse_text(&broken).err();
    eframe::App::on_exit(&mut w.app);
    assert_eq!(w.disk(), before, "the settings file is not touched");
    assert_eq!(std::fs::read_to_string(rec.join(&day).join("settings/config.toml")).unwrap(), broken);
    assert_eq!(crate::recovery::unseen(&rec), vec![rec.join(&day)], "announced at the next start");

    // valid text typed just before quitting: saved, nothing rescued
    let mut w = W::new("exit-valid");
    let rec = w.dir.join("recovery-root");
    w.app.recovery.root = rec.clone();
    w.app.open_settings_file();
    w.app.sf.text = w.disk().replace("toolbar_size = 110", "toolbar_size = 17");
    w.app.sf.dirty = true;
    w.app.sf.last_edit = Instant::now();
    eframe::App::on_exit(&mut w.app);
    assert!(w.disk().contains("toolbar_size = 17"));
    assert!(!rec.exists(), "a clean exit rescues nothing");

    // the file changed outside while the tab kept its own text: the tab's text is rescued
    let mut w = W::new("exit-outside");
    let rec = w.dir.join("recovery-root");
    w.app.recovery.root = rec.clone();
    w.app.open_settings_file();
    let mine = w.disk().replace("toolbar_size = 110", "toolbar_size = 19");
    w.app.sf.text = mine.clone();
    w.app.sf.dirty = true;
    w.app.sf.last_edit = Instant::now();
    w.write_outside(&w.disk().replace("toolbar_size = 110", "toolbar_size = 21"));
    w.app.check_settings_file(true);
    assert!(w.app.sf.disk_changed);
    eframe::App::on_exit(&mut w.app);
    assert!(w.disk().contains("toolbar_size = 21"), "the other program's file stays");
    assert_eq!(std::fs::read_to_string(rec.join(&day).join("settings/config.toml")).unwrap(), mine);
}

// spec: EDIT-039, EDIT-040, EDIT-041
#[test]
fn edit_039_the_cursor_settings_are_in_the_file_and_unknown_words_are_the_default() {
    use crate::cursor_style::CursorShape;
    let mut w = W::new("cursor");
    w.app.open_settings_file();
    assert_eq!((w.app.cfg.cursor_shape, w.app.cfg.normal_cursor_shape, w.app.cfg.cursor_blink), (CursorShape::Bar, CursorShape::Block, true));
    assert!(w.app.sf.text.contains("cursor_shape = \"bar\"") && w.app.sf.text.contains("normal_cursor_shape = \"block\"") && w.app.sf.text.contains("cursor_blink = true"), "{}", w.app.sf.text);
    // a change in the tab is used at once and round-trips
    w.app.sf.text = w.app.sf.text.replace("cursor_shape = \"bar\"", "cursor_shape = \"underline\"").replace("normal_cursor_shape = \"block\"", "normal_cursor_shape = \"bar_after\"").replace("cursor_blink = true", "cursor_blink = false");
    w.app.sf.dirty = true;
    assert!(w.app.save_settings_file(true));
    assert_eq!((w.app.cfg.cursor_shape, w.app.cfg.normal_cursor_shape, w.app.cfg.cursor_blink), (CursorShape::Underline, CursorShape::BarAfter, false));
    assert!(w.disk().contains("cursor_shape = \"underline\"") && w.disk().contains("normal_cursor_shape = \"bar_after\""));
    // an unknown word is the default of that setting, and the rest of the file is still read
    w.app.sf.text =
        w.app.sf.text.replace("cursor_shape = \"underline\"", "cursor_shape = \"squiggle\"").replace("normal_cursor_shape = \"bar_after\"", "normal_cursor_shape = \"squiggle\"").replace("toolbar_size = 110", "toolbar_size = 19");
    w.app.sf.dirty = true;
    assert!(w.app.save_settings_file(true));
    assert_eq!((w.app.cfg.cursor_shape, w.app.cfg.normal_cursor_shape, w.app.cfg.toolbar_size), (CursorShape::Bar, CursorShape::Block, 19));
}

// spec: EDIT-043
#[test]
fn edit_043_esc_cursor_is_in_the_file_and_an_unknown_word_is_the_default() {
    use crate::vim::EscCursor;
    let mut w = W::new("esc-cursor");
    w.app.open_settings_file();
    assert_eq!(w.app.cfg.esc_cursor, EscCursor::Stay);
    assert!(w.app.sf.text.contains("esc_cursor = \"stay\""), "{}", w.app.sf.text);
    w.app.sf.text = w.app.sf.text.replace("esc_cursor = \"stay\"", "esc_cursor = \"step_back\"");
    w.app.sf.dirty = true;
    assert!(w.app.save_settings_file(true));
    assert_eq!(w.app.cfg.esc_cursor, EscCursor::StepBack);
    assert!(w.disk().contains("esc_cursor = \"step_back\""));
    w.app.sf.text = w.app.sf.text.replace("esc_cursor = \"step_back\"", "esc_cursor = \"sideways\"").replace("toolbar_size = 110", "toolbar_size = 19");
    w.app.sf.dirty = true;
    assert!(w.app.save_settings_file(true));
    assert_eq!((w.app.cfg.esc_cursor, w.app.cfg.toolbar_size), (EscCursor::Stay, 19));
}
