//! Settings → Sync, the git part (SYNC-023…SYNC-029), driven through the whole window, headless: what
//! is found and what is in use, the offer per platform with its sizes and nothing downloaded before the
//! button, the progress and Cancel, update, remove, the choice and the opt-in check. The server is a
//! pretend GitHub on 127.0.0.1 and the data folder a temporary one; the jobs are the real ones.

use super::install_tests::{publish_newer_for_ui, World};
use super::install_ui::*;
use super::managed::*;
use super::sources::*;
use crate::app::{Options, SettingsTab, ShaleApp};
use crate::ui_test_support::Headless;
use eframe::egui::PointerButton;
use std::ffi::OsString;
use std::time::Duration;

struct T {
    app: ShaleApp,
    hl: Headless,
    w: World,
    /// A folder that is the whole `PATH` of the pretend computer.
    path: OsString,
}

fn found(git: Option<&str>, lfs: Option<&str>) -> Detection {
    Detection { system_git: git.map(String::from), system_lfs: lfs.map(String::from), managed: None, git_in_use: git.map(|g| (g.to_string(), Origin::System)), lfs_in_use: lfs.map(|l| (l.to_string(), Origin::System)) }
}

impl T {
    fn new(tag: &str, os: &str, arch: &str, detection: Detection) -> T {
        let w = World::new(tag, "2.99.0", "9.9.9");
        let path = OsString::from(w.data.join("no-bin"));
        let mut app = ShaleApp::build(Options { screenshot: None, vault: None, view: None, keys: None }, false);
        app.sync.managed = ManagedState::for_test(w.data.clone(), Some(path.clone()), w.sources.clone(), Platform::new(os, arch));
        app.sync.managed.set_detection(detection, GitChoice::Auto);
        app.open_settings(SettingsTab::Sync);
        let mut t = T { app, hl: Headless::new(), w, path };
        t.hl.set_size(eframe::egui::Vec2::new(1500.0, 1400.0));
        t.hl.frames(&mut t.app, 4);
        t
    }

    fn shows(&mut self, text: &str) -> bool {
        self.hl.frames(&mut self.app, 3);
        self.hl.text.iter().any(|(t, _)| t.contains(text))
    }

    fn painted(&self) -> Vec<&str> {
        self.hl.text.iter().map(|(t, _)| t.as_str()).collect()
    }

    fn click(&mut self, label: &str) {
        let r = self.hl.find_label(&mut self.app, label);
        self.hl.click(&mut self.app, r.center(), PointerButton::Primary);
    }

    fn wait_for_job(&mut self) {
        for _ in 0..400 {
            self.hl.frames(&mut self.app, 1);
            let m = &self.app.sync.managed;
            if m.task.is_none() && m.detection.is_some() {
                self.hl.frames(&mut self.app, 3);
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the job did not end");
    }

    fn root(&self) -> std::path::PathBuf {
        managed_root(&self.w.data)
    }
}

// spec: SYNC-023
#[test]
fn the_page_says_what_was_found_and_what_is_in_use() {
    let mut t = T::new("found", "linux", "x86_64", found(Some("git version 2.47.0"), None));
    for want in [
        "Git sync",
        "git of this computer: git version 2.47.0",
        "git-lfs of this computer: not found",
        "Shale's own git: not installed",
        "In use: git version 2.47.0 (this computer's), no git-lfs",
        "Automatic",
        "The git of this computer",
        "The git that Shale installs",
        "Check again",
    ] {
        assert!(t.shows(want), "{want:?} missing in {:?}", t.painted());
    }
    let d = Detection { git_in_use: Some(("git version 2.56.1".into(), Origin::Managed)), lfs_in_use: Some(("git-lfs/3.8.1".into(), Origin::Managed)), ..found(None, None) };
    t.app.sync.managed.set_detection(d, GitChoice::Auto);
    assert!(t.shows("git of this computer: not found") && t.shows("In use: git version 2.56.1 (Shale's), git-lfs/3.8.1 (Shale's)"), "{:?}", t.painted());
    t.app.sync.managed.set_detection(found(None, None), GitChoice::Auto);
    assert!(t.shows("No git can be used."), "{:?}", t.painted());
    assert!(t.app.sync.managed.task.is_none(), "looking at the page starts no download");
}

// spec: SYNC-025, SYNC-029
#[test]
fn without_git_windows_offers_the_download_with_sizes_and_the_folder_and_downloads_nothing_before_the_button() {
    let mut t = T::new("offer", "windows", "x86_64", found(None, None));
    let text = t.painted().join("\n");
    assert!(t.shows("Download and install git"), "{:?}", t.painted());
    let text = format!("{text}\n{}", t.painted().join("\n"));
    assert!(text.contains("git 2.99.0, git-lfs 9.9.9") && text.contains("from github.com") && text.contains("on disk"), "{text}");
    assert!(text.contains(&*t.root().to_string_lossy()), "the target folder: {text}");
    assert!(text.contains("Nothing is downloaded before you press the button"), "{text}");
    t.hl.frames(&mut t.app, 5);
    assert!(t.app.sync.managed.task.is_none() && t.w.files.paths().is_empty(), "nothing was asked for: {:?}", t.w.files.paths());
    assert!(!t.root().exists());
}

// spec: SYNC-025, SYNC-024
#[test]
fn the_button_installs_in_a_job_with_progress_and_the_window_runs_the_new_git_from_then_on() {
    let mut t = T::new("install", "windows", "x86_64", found(None, None));
    t.app.sync.managed.select = true;
    t.click("Download and install git");
    assert!(t.app.sync.managed.task.as_ref().is_some_and(|k| k.kind == TaskKind::Install) || t.app.sync.managed.notice.is_some(), "the install ran as a job");
    t.wait_for_job();
    let n = t.app.sync.managed.notice.clone().expect("a notice");
    assert!(n.ok && n.text.contains("The install of git and git-lfs is done: git 2.99.0, git-lfs 9.9.9"), "{n:?}");
    assert!(t.root().join("cmd/git.exe").is_file());
    // the window now runs the managed git (the PATH of the pretend computer has none)
    assert_eq!(t.app.sync.git.program, t.root().join("cmd/git.exe"));
    assert!(t.shows("Shale's own git: git 2.99.0, git-lfs 9.9.9"), "{:?}", t.painted());
    assert!(t.shows("Remove…") && t.shows("Look for a new version") && !t.shows("Download and install git"), "{:?}", t.painted());
    let _ = &t.path;
}

// spec: SYNC-025
#[test]
fn a_failed_install_says_why_and_leaves_nothing() {
    let mut t = T::new("fails", "windows", "x86_64", found(None, None));
    t.w.files.map_clear();
    t.click("Download and install git");
    t.wait_for_job();
    let n = t.app.sync.managed.notice.clone().expect("a notice");
    assert!(!n.ok && n.text.contains("The install of git and git-lfs failed"), "{n:?}");
    assert!(t.shows("The install of git and git-lfs failed"), "the page says so: {:?}", t.painted());
    assert!(t.w.leftovers().is_empty());
    assert!(t.shows("Download and install git"), "the offer is back");
}

// spec: SYNC-025
#[test]
fn the_progress_of_a_running_install_is_shown_and_cancel_asks_the_job_to_stop() {
    let mut t = T::new("progress", "windows", "x86_64", found(None, None));
    let handle = super::install::Handle::default();
    let (_tx, rx) = std::sync::mpsc::channel();
    t.app.sync.managed.task = Some(Task { kind: TaskKind::Install, handle: handle.clone(), rx });
    assert!(t.shows("Cancel install") && !t.shows("Download and install git"), "no second install while one runs: {:?}", t.painted());
    assert!(!handle.cancelled());
    t.click("Cancel install");
    assert!(handle.cancelled(), "Cancel stops the job");
    let (_tx2, rx) = std::sync::mpsc::channel();
    t.app.sync.managed.task = Some(Task { kind: TaskKind::Update, handle: super::install::Handle::default(), rx });
    assert!(t.shows("Cancel update") && !t.shows("Cancel install"));
    // the end of the job is taken by the window
    let (tx, rx) = std::sync::mpsc::channel();
    t.app.sync.managed.task = Some(Task { kind: TaskKind::Update, handle: super::install::Handle::default(), rx });
    tx.send(Ok(Notice { text: "The update is done: x.".into(), ok: true })).unwrap();
    assert!(t.shows("The update is done: x."), "{:?}", t.painted());
    assert!(t.app.sync.managed.task.is_none());
}

fn installed(t: &mut T) {
    super::install::install(&t.w.env(), &t.w.plan("windows", "x86_64"), &super::install::Handle::default(), &|| {}).unwrap();
    let d = detect(GitChoice::Auto, Some(&t.w.data), Some(&t.path));
    t.app.sync.managed.set_detection(d, GitChoice::Auto);
}

// spec: SYNC-028
#[test]
fn remove_asks_first_naming_the_folder_and_removes_that_folder_only_after_the_confirmation() {
    let mut t = T::new("remove", "windows", "x86_64", found(None, None));
    installed(&mut t);
    std::fs::write(t.w.data.join("shale").join("keep.txt"), "mine").unwrap();
    t.app.sync.managed.select = true;
    assert!(t.shows(&t.root().to_string_lossy()), "{:?}", t.painted());
    t.click("Remove…");
    assert!(t.app.sync.managed.confirm_remove && t.root().exists(), "pressing Remove only asks");
    let text = t.painted().join("\n");
    assert!(t.shows("This deletes the folder") && text.contains("Nothing else is removed") || t.shows("Nothing else is removed"), "{:?}", t.painted());
    t.click("Keep it");
    assert!(!t.app.sync.managed.confirm_remove && t.root().exists());
    t.click("Remove…");
    t.click("Remove Shale's git folder");
    t.wait_for_job();
    assert!(!t.root().exists(), "the folder is gone");
    assert!(t.w.data.join("shale").join("keep.txt").is_file(), "nothing else was removed");
    assert!(t.shows("Shale's git was removed."), "{:?}", t.painted());
    assert_eq!(t.app.sync.git.program, std::path::PathBuf::from("git"), "the window's git is selected again");
}

// spec: SYNC-024, CFG-054
#[test]
fn the_git_choice_is_a_setting_changed_by_a_click_and_selects_another_git() {
    let mut t = T::new("choice", "windows", "x86_64", found(None, None));
    installed(&mut t);
    t.app.sync.managed.select = true;
    t.app.refresh_git();
    assert_eq!(t.app.cfg.git.choice, GitChoice::Auto);
    t.hl.frames(&mut t.app, 3);
    assert_eq!(t.app.sync.git.program, t.root().join("cmd/git.exe"), "Auto: no system git, so the managed one");
    t.click("The git of this computer");
    assert_eq!(t.app.cfg.git.choice, GitChoice::System);
    t.hl.frames(&mut t.app, 3);
    assert_eq!(t.app.sync.git.program, std::path::PathBuf::from("git"), "System: only the git of PATH, none here");
    t.click("The git that Shale installs");
    assert_eq!(t.app.cfg.git.choice, GitChoice::Managed);
    t.hl.frames(&mut t.app, 3);
    assert_eq!(t.app.sync.git.program, t.root().join("cmd/git.exe"));
    t.click("Automatic");
    assert_eq!(t.app.cfg.git.choice, GitChoice::Auto);
    // the page looked again for the new choice
    t.wait_for_job();
    assert!(t.app.sync.managed.detection.is_some());
}

// spec: SYNC-029
#[test]
fn macos_without_git_explains_apples_installer_and_linux_shows_the_command_of_the_distribution() {
    let mut t = T::new("mac", "macos", "aarch64", found(None, None));
    assert!(t.shows("Install git (Apple's command line tools)") && t.shows("There is no git on this Mac"), "{:?}", t.painted());
    assert!(!t.shows("Download and install"), "nothing is downloaded for git on a Mac");
    let mut t = T::new("linux", "linux", "x86_64", found(None, None));
    t.app.sync.managed.os_release = Some("ID=fedora\nNAME=\"Fedora Linux\"\n".into());
    assert!(t.shows("sudo dnf install git git-lfs") && t.shows("Copy command") && t.shows("On Fedora Linux install it"), "{:?}", t.painted());
    assert!(!t.shows("Download and install"));
    // git there, git-lfs missing: the download of git-lfs alone
    t.app.sync.managed.set_detection(found(Some("git version 2.47.0"), None), GitChoice::Auto);
    assert!(t.shows("Download and install git-lfs") && t.shows("about"), "{:?}", t.painted());
    // an unknown system
    let mut t = T::new("odd", "plan9", "mips", found(None, None));
    assert!(t.shows("Shale has no download of git for plan9 on mips."), "{:?}", t.painted());
}

// spec: SYNC-027
#[test]
fn a_found_update_is_offered_and_installed_by_one_button() {
    let mut t = T::new("update-ui", "windows", "x86_64", found(None, None));
    installed(&mut t);
    let plan = publish_newer_for_ui(&t.w, "2.100.0", "9.10.0");
    t.app.sync.managed.update = Some(plan);
    t.app.sync.managed.select = true;
    assert!(t.shows("A newer version is out: git 2.100.0, git-lfs 9.10.0"), "{:?}", t.painted());
    t.click("Update");
    t.wait_for_job();
    let n = t.app.sync.managed.notice.clone().unwrap();
    assert!(n.ok && n.text.contains("The update is done: git 2.100.0, git-lfs 9.10.0"), "{n:?}");
    assert!(t.app.sync.managed.update.is_none());
    assert_eq!(Installed::read(&t.root()).unwrap().versions_text(), "git 2.100.0, git-lfs 9.10.0");
    assert!(!t.shows("A newer version is out"));
}

fn api_requests(t: &T) -> usize {
    t.w.files.paths().iter().filter(|p| p.starts_with("/repos/")).count()
}

// spec: SYNC-027, CFG-054
#[test]
fn the_check_for_a_new_version_is_opt_in_and_the_setting_decides_what_happens_to_a_new_one() {
    let mut t = T::new("opt-in", "windows", "x86_64", found(None, None));
    installed(&mut t);
    publish_newer_for_ui(&t.w, "2.100.0", "9.10.0");
    t.app.sync.managed.checks = true;
    t.app.sync.managed.check_is_due();
    // Off, the default: no request is made
    assert_eq!(t.app.cfg.git.updates, GitUpdates::Off);
    t.hl.frames(&mut t.app, 10);
    assert_eq!(api_requests(&t), 0, "{:?}", t.w.files.paths());
    // the page says what the setting does and has the three choices
    for want in ["Never look for a new version", "Look for a new version and tell me", "Look for a new version and install it", "Off sends no request"] {
        assert!(t.shows(want), "{want:?} missing in {:?}", t.painted());
    }
    // Notify: the check runs, says so, and only offers
    t.click("Look for a new version and tell me");
    assert_eq!(t.app.cfg.git.updates, GitUpdates::Notify);
    t.app.sync.managed.check_is_due();
    for _ in 0..200 {
        t.hl.frames(&mut t.app, 1);
        if t.app.sync.managed.update.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(t.app.sync.managed.update.is_some(), "the newer release was found");
    assert!(t.app.toasts.iter().any(|(m, _)| m.contains("A newer git is available (git 2.100.0, git-lfs 9.10.0)")), "{:?}", t.app.toasts);
    assert!(t.app.sync.managed.task.is_none(), "Notify installs nothing");
    assert_eq!(Installed::read(&t.root()).unwrap().versions_text(), "git 2.99.0, git-lfs 9.9.9");
    // Install: the next release is installed at once
    t.app.sync.managed.update = None;
    t.click("Look for a new version and install it");
    assert_eq!(t.app.cfg.git.updates, GitUpdates::Install);
    t.app.sync.managed.check_is_due();
    t.wait_for_job();
    for _ in 0..200 {
        if Installed::read(&t.root()).is_some_and(|i| i.versions_text() == "git 2.100.0, git-lfs 9.10.0") {
            break;
        }
        t.hl.frames(&mut t.app, 1);
        std::thread::sleep(Duration::from_millis(20));
    }
    t.wait_for_job();
    assert_eq!(Installed::read(&t.root()).unwrap().versions_text(), "git 2.100.0, git-lfs 9.10.0", "{:?}", t.painted());
}

// spec: SYNC-027
#[test]
fn the_button_looks_for_a_new_version_now_and_says_when_there_is_none() {
    let mut t = T::new("look", "windows", "x86_64", found(None, None));
    installed(&mut t);
    publish_newer_for_ui(&t.w, "2.99.0", "9.9.9");
    t.click("Look for a new version");
    for _ in 0..200 {
        t.hl.frames(&mut t.app, 1);
        if t.app.sync.managed.notice.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(t.shows("The git that Shale installed is up to date."), "{:?}", t.painted());
    assert_eq!(api_requests(&t), 2, "one per project");
}
