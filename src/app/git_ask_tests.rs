//! The question about a folder with its own `.git` (SYNC-076): asked when it is opened or added,
//! answered by a click, and no sync before the answer.

use super::vault_load::After;
use super::{Options, ShaleApp};
use crate::config::testing::TestConfig;
use crate::gitsync;
use crate::ui_test_support::Headless;
use eframe::egui::{Event, Key, Modifiers, PointerButton};
use std::path::PathBuf;

struct W {
    app: ShaleApp,
    hl: Headless,
    base: PathBuf,
    _cfg: TestConfig,
}

impl W {
    /// A window with no vault and a settings file of its own; the folders `plain` (no git),
    /// `repo` (its own `.git`), `made` (a `.git` Shale made) and `parent/child` (inside a
    /// repository) are made.
    fn new(tag: &str) -> W {
        let cfg = TestConfig::new(tag);
        let base = std::env::temp_dir().join(format!("shale-git-ask-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for name in ["plain", "repo", "made", "parent/child"] {
            std::fs::create_dir_all(base.join(name)).unwrap();
            std::fs::write(base.join(name).join("Welcome.md"), format!("# {name}\n")).unwrap();
        }
        let git = gitsync::Git { program: "git".into(), env: vec![("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()), ("GIT_CONFIG_NOSYSTEM".into(), "1".into())] };
        for name in ["repo", "parent"] {
            assert!(std::process::Command::new("git").arg("-C").arg(base.join(name)).args(["init", "--quiet"]).envs(git.env.iter().cloned()).status().unwrap().success());
        }
        let remote = base.join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        assert!(std::process::Command::new("git").args(["init", "--quiet", "--bare"]).arg(&remote).envs(git.env.iter().cloned()).status().unwrap().success());
        gitsync::connect(&git, &base.join("made"), remote.to_str().unwrap()).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: None, view: None, keys: None }, false);
        app.persist_config = true;
        let mut w = W { app, hl: Headless::new(), base, _cfg: cfg };
        w.hl.frames(&mut w.app, 3);
        w
    }

    fn dir(&self, name: &str) -> PathBuf {
        self.base.join(name)
    }

    fn open(&mut self, name: &str, after: After) {
        self.app.open_vault_in_background(&self.dir(name), after);
        self.hl.settle(&mut self.app);
        self.hl.frames(&mut self.app, 3);
    }

    fn painted(&self, text: &str) -> bool {
        self.hl.text.iter().any(|(t, _)| t == text)
    }

    fn click(&mut self, label: &str) {
        let r = self.hl.find_label(&mut self.app, label);
        self.hl.click(&mut self.app, r.center(), PointerButton::Primary);
    }

    fn esc(&mut self) {
        let ev = |pressed| Event::Key { key: Key::Escape, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        self.hl.frame(&mut self.app, vec![ev(true)]);
        self.hl.frame(&mut self.app, vec![ev(false)]);
        self.hl.frames(&mut self.app, 3);
    }

    fn asking(&self) -> bool {
        self.app.sync.repo_ask.is_some()
    }

    /// Frames, then whether no sync was started or tried (the tick leaves `last_start` alone).
    fn no_sync_runs(&mut self) -> bool {
        self.app.sync.last_start = None;
        self.hl.frames(&mut self.app, 5);
        !self.app.sync.running() && self.app.sync.last_start.is_none()
    }

    fn settings_on_disk(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir(name).join(".shale/settings.json")).unwrap_or_default()
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

// spec: SYNC-076
#[test]
fn sync_076_a_folder_with_its_own_git_asks_whether_to_use_git_sync_and_nothing_syncs_before_the_answer() {
    let mut w = W::new("asks");
    w.open("repo", After::Nothing);
    assert!(w.asking() && w.app.vault.is_some());
    assert!(w.painted("Use git sync?") && w.painted("Use git sync") && w.painted("Don't sync"), "{:?}", w.hl.text.iter().map(|t| &t.0).collect::<Vec<_>>());
    assert!(w.hl.text.iter().any(|(t, _)| t.contains("auto sync") && t.contains("committed automatically") && t.contains("pushed")), "the dialog says what git sync does");
    assert!(w.app.shortcuts_blocked(), "the keys belong to the question");
    assert!(w.app.vdata.settings.sync.enabled, "sync.enabled is untouched until the answer");
    assert!(w.no_sync_runs(), "no sync runs before the answer");
    assert!(!w.app.start_sync_with(w.app.sync.git.clone()), "not even Sync now");
    assert_eq!(w.app.cfg.vaults.entries().len(), 1, "the folder was added to the known vaults (VAULT-029)");
}

// spec: SYNC-076
#[test]
fn sync_076_use_git_sync_keeps_sync_on_remembers_the_answer_and_syncs() {
    let mut w = W::new("use");
    w.open("repo", After::Nothing);
    w.click("Use git sync");
    assert!(!w.asking() && !w.painted("Use git sync?") && !w.app.shortcuts_blocked());
    assert!(w.app.vdata.settings.sync.enabled && !w.settings_on_disk("repo").contains("\"enabled\": false"));
    assert!(gitsync::known::is_known(&w.dir("repo")));
    // the sync is due again at once
    assert!(w.app.sync.last_start.is_some(), "the sync was tried: {:?}", w.app.sync.last);
    // the next open does not ask again
    w.open("plain", After::Nothing);
    w.open("repo", After::Nothing);
    assert!(!w.asking());
}

// spec: SYNC-076
#[test]
fn sync_076_dont_sync_turns_sync_off_in_the_vault_settings_and_the_next_open_does_not_ask() {
    let mut w = W::new("dont");
    w.open("repo", After::Nothing);
    w.click("Don't sync");
    assert!(!w.asking() && !w.app.vdata.settings.sync.enabled);
    assert!(w.settings_on_disk("repo").contains("\"enabled\": false"), "{}", w.settings_on_disk("repo"));
    assert!(gitsync::known::is_known(&w.dir("repo")));
    assert!(w.no_sync_runs());
    w.open("plain", After::Nothing);
    w.open("repo", After::Nothing);
    assert!(!w.asking() && !w.app.vdata.settings.sync.enabled, "the answer stays");
}

// spec: SYNC-076
#[test]
fn sync_076_esc_decides_later_nothing_syncs_and_the_next_open_asks_again() {
    let mut w = W::new("esc");
    w.open("repo", After::Nothing);
    w.esc();
    assert!(w.asking() && !w.painted("Use git sync?"), "the dialog is gone");
    assert!(!w.app.shortcuts_blocked());
    assert!(!gitsync::known::is_known(&w.dir("repo")) && w.app.vdata.settings.sync.enabled, "nothing was decided");
    assert!(w.no_sync_runs() && !w.app.start_sync_with(w.app.sync.git.clone()), "still no sync without an answer");
    w.open("plain", After::Nothing);
    assert!(!w.asking(), "another vault is not asked about");
    w.open("repo", After::Nothing);
    assert!(w.asking() && w.painted("Use git sync?"), "asked again");
}

// spec: SYNC-076
#[test]
fn sync_076_no_question_without_a_git_of_its_own_or_for_one_shale_made() {
    let mut w = W::new("none");
    w.open("plain", After::Nothing);
    assert!(!w.asking(), "no .git");
    w.open("made", After::Nothing);
    assert!(!w.asking() && w.app.vault.is_some(), "a .git Shale made (SYNC-004)");
    w.open("parent/child", After::Nothing);
    assert!(!w.asking() && w.app.vault.is_some(), "a repository in a parent folder is not used (SYNC-002)");
    assert!(!gitsync::is_repo(&w.dir("parent/child")));
}

// spec: SYNC-076, VAULT-029
#[test]
fn sync_076_the_user_guide_and_a_window_that_does_not_save_never_ask() {
    let mut w = W::new("guide");
    // the guide is not a known vault, also when its folder has a .git
    w.open("repo", After::Guide(PathBuf::from("Welcome.md"), None));
    assert!(!w.asking() && w.app.cfg.vaults.entries().is_empty());
    // a window that saves nothing (screenshots, tests) has no sync and asks nothing
    w.app.persist_config = false;
    w.open("repo", After::Nothing);
    assert!(!w.asking());
}
