//! A second start of Shale hands its folder or file to the running one (DATA-015): the window
//! side, in the headless whole window. The socket side is tested in `instance.rs`.

use super::{Options, ShaleApp, View};
use crate::instance;
use crate::ui_test_support::Headless;
use std::path::{Path, PathBuf};

struct W {
    app: ShaleApp,
    hl: Headless,
    base: PathBuf,
    data: PathBuf,
}

impl W {
    fn new(tag: &str) -> W {
        let base = std::env::temp_dir().join(format!("shale-handover-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let vault = base.join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::write(vault.join("Welcome.md"), "# Welcome\n").unwrap();
        std::fs::write(vault.join("Other.md"), "# Other\n").unwrap();
        std::fs::create_dir_all(base.join("outside")).unwrap();
        std::fs::write(base.join("outside/x.md"), "# X\n").unwrap();
        std::fs::create_dir_all(base.join("vault2")).unwrap();
        std::fs::write(base.join("vault2/Two.md"), "# Two\n").unwrap();
        let data = base.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(vault), view: None, keys: None }, false);
        app.handover = Some(instance::listen(&data).unwrap());
        let mut w = W { app, hl: Headless::new(), base, data };
        w.hl.frames(&mut w.app, 3);
        w
    }

    /// What a second start does, then frames until the window has taken it.
    fn second_start(&mut self, paths: &[PathBuf]) {
        assert!(instance::hand_over(&self.data, paths));
        for _ in 0..200 {
            self.hl.frame(&mut self.app, vec![]);
            if !self.toasts().is_empty() || self.active_note().is_some_and(|n| n != Path::new("Welcome.md")) || self.vault_root().ends_with("vault2") {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn active_note(&self) -> Option<PathBuf> {
        match &self.app.tabbar.tabs[self.app.tabbar.active].view {
            View::Note(p) => Some(p.clone()),
            _ => None,
        }
    }

    fn vault_root(&self) -> PathBuf {
        self.app.vault.as_ref().unwrap().root.clone()
    }

    fn toasts(&self) -> Vec<String> {
        self.app.toasts.iter().map(|t| t.0.clone()).collect()
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

// spec: DATA-015
#[test]
fn a_file_of_the_open_vault_opens_in_a_tab() {
    let mut w = W::new("file");
    let tabs = w.app.tabbar.tabs.len();
    w.second_start(&[w.base.join("vault/Other.md")]);
    assert_eq!(w.active_note().as_deref(), Some(Path::new("Other.md")));
    assert_eq!(w.app.tabbar.tabs.len(), tabs + 1);
}

// spec: DATA-015
#[test]
fn a_folder_is_opened_as_the_vault_and_the_open_one_is_left_alone() {
    let mut w = W::new("folder");
    let tabs = w.app.tabbar.tabs.len();
    w.second_start(&[w.base.join("vault")]);
    assert!(w.vault_root().ends_with("vault") && w.app.tabbar.tabs.len() == tabs, "the same folder changes nothing");
    w.second_start(&[w.base.join("vault2")]);
    assert!(w.vault_root().ends_with("vault2"));
}

// spec: DATA-015
#[test]
fn a_file_outside_the_vault_is_said_so_and_a_start_without_paths_only_comes_forward() {
    let mut w = W::new("outside");
    w.second_start(&[w.base.join("outside/x.md")]);
    assert!(w.toasts().iter().any(|t| t.contains("is not in the open vault")), "{:?}", w.toasts());
    let before = w.app.tabbar.tabs.len();
    assert!(instance::hand_over(&w.data, &[]));
    w.hl.frames(&mut w.app, 30);
    assert_eq!(w.app.tabbar.tabs.len(), before);
}
