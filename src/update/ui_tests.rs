//! The updates in the window, driven headless (UPD-007 to UPD-010, UPD-019, UPD-020): the question
//! which mode to use, the buttons of the notice, the release notes and Settings → Updates. The
//! check, download and install are the counting fakes of `tests.rs`.

use super::logic_tests::{fake, gated, portable_kind, state, temp, Counts};
use super::*;
use crate::app::{Options, SettingsTab, ShaleApp};
use crate::ui_test_support::Headless;
use eframe::egui::{self, Event, PointerButton, ViewportCommand};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

struct T {
    app: ShaleApp,
    hl: Headless,
    counts: Counts,
}

impl T {
    fn new(mode: UpdateMode, latest: Option<&str>, kind: InstallKind) -> T {
        T::prepared(mode, latest, kind, |_, _| {})
    }

    /// Like `new`, with `prepare` run before the first frame (the first check starts in it).
    fn prepared(mode: UpdateMode, latest: Option<&str>, kind: InstallKind, prepare: impl FnOnce(&mut ShaleApp, &Counts)) -> T {
        let dir = temp("ui-vault");
        crate::vault::write_sample_vault(&dir).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir), view: None, keys: None }, false);
        let (backend, counts) = fake(latest);
        app.update = state(kind, backend);
        app.update.busy = app.busy.clone();
        app.cfg.updates.mode = mode;
        prepare(&mut app, &counts);
        let mut t = T { app, hl: Headless::new(), counts };
        t.hl.set_size(egui::Vec2::new(1500.0, 1100.0));
        t.hl.frames(&mut t.app, 3);
        t
    }

    /// Frames, with a rest between them, until `done`.
    fn settle(&mut self, done: impl Fn(&T) -> bool) {
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            self.hl.frames(&mut self.app, 2);
            if done(self) {
                self.hl.frames(&mut self.app, 3);
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the window did not settle: {:?} {:?}", self.app.update.phase, self.app.update.result);
    }

    fn shows(&mut self, text: &str) -> bool {
        self.hl.frames(&mut self.app, 3);
        self.hl.text.iter().any(|(t, _)| t.contains(text))
    }

    /// Click the widget with this label; the output of the frame in which the button was let go.
    fn click(&mut self, label: &str) -> egui::FullOutput {
        let p = self.hl.find_label(&mut self.app, label).center();
        self.hl.frame(&mut self.app, vec![Event::PointerMoved(p)]);
        self.hl.frame(&mut self.app, vec![Headless::button(p, PointerButton::Primary, true)]);
        let out = self.hl.frame(&mut self.app, vec![Headless::button(p, PointerButton::Primary, false)]);
        self.hl.frames(&mut self.app, 3);
        out
    }

    fn painted(&self) -> Vec<&str> {
        self.hl.text.iter().map(|(t, _)| t.as_str()).collect()
    }
}

// spec: UPD-007
#[test]
fn upd_007_the_first_update_found_asks_which_mode_to_use_and_the_choice_is_saved_and_used() {
    let mut t = T::new(UpdateMode::Ask, Some("9.9.9"), portable_kind());
    t.settle(|t| t.app.update.ask);
    assert!(t.shows("What should Shale do about updates from now on?"), "{:?}", t.painted());
    for line in UpdateMode::CHOICES.map(|m| m.about()) {
        assert!(t.shows(line), "{line}");
    }
    assert_eq!(Counts::n(&t.counts.fetches), 0, "nothing is downloaded before the answer");
    t.click("Download");
    assert_eq!(t.app.cfg.updates.mode, UpdateMode::Download, "the choice is stored");
    assert!(!t.app.update.ask && !t.shows("What should Shale do about updates"), "the question is gone");
    t.settle(|t| matches!(t.app.update.phase, Phase::Ready(_)));
    assert!(t.shows("Shale 9.9.9 is downloaded and checked, ready to install."), "{:?}", t.painted());
    assert_eq!(Counts::n(&t.counts.fetches), 1);
}

// spec: UPD-007
#[test]
fn upd_007_escape_leaves_the_question_for_later_and_choosing_off_ends_the_checks() {
    let mut t = T::new(UpdateMode::Ask, Some("9.9.9"), portable_kind());
    t.settle(|t| t.app.update.ask);
    t.hl.frame(&mut t.app, vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
    t.hl.frames(&mut t.app, 3);
    assert!(!t.app.update.ask && t.app.cfg.updates.mode == UpdateMode::Ask, "nothing was chosen");
    let mut t = T::new(UpdateMode::Ask, Some("9.9.9"), portable_kind());
    t.settle(|t| t.app.update.ask);
    t.click("Off");
    assert_eq!(t.app.cfg.updates.mode, UpdateMode::Off);
    assert!(!t.shows("Shale 9.9.9 is available"), "nothing is shown");
}

// spec: UPD-008
#[test]
fn upd_008_the_notice_of_notify_has_its_buttons_and_skip_and_later_take_it_away() {
    let mut t = T::new(UpdateMode::Notify, Some("9.9.9"), portable_kind());
    t.settle(|t| t.app.update.found.is_some());
    assert!(t.shows("Shale 9.9.9 is available."), "{:?}", t.painted());
    for b in ["What's new", "Download", "Install", "Skip this version", "Later"] {
        t.hl.find_label(&mut t.app, b);
    }
    t.click("Later");
    assert!(!t.shows("Shale 9.9.9 is available."));
    assert_eq!(t.app.cfg.updates.skipped_version, "", "Later saves nothing");
    let mut t = T::new(UpdateMode::Notify, Some("9.9.9"), portable_kind());
    t.settle(|t| t.app.update.found.is_some());
    t.click("Skip this version");
    assert_eq!(t.app.cfg.updates.skipped_version, "9.9.9");
    assert!(!t.shows("Shale 9.9.9 is available."));
}

// spec: UPD-020
#[test]
fn upd_020_whats_new_shows_the_release_notes() {
    let mut t = T::new(UpdateMode::Notify, Some("9.9.9"), portable_kind());
    t.settle(|t| t.app.update.found.is_some());
    t.click("What's new");
    assert!(t.shows("What's new in Shale 9.9.9") && t.shows("Notes of 9.9.9"), "{:?}", t.painted());
    t.click("Close");
    assert!(!t.shows("Notes of 9.9.9"));
}

// spec: UPD-009
#[test]
fn upd_009_the_buttons_download_install_now_and_restart_now_lead_through_the_update() {
    let mut t = T::new(UpdateMode::Notify, Some("9.9.9"), portable_kind());
    t.settle(|t| t.app.update.found.is_some());
    t.click("Download");
    t.settle(|t| matches!(t.app.update.phase, Phase::Ready(_)));
    assert!(t.shows("ready to install") && Counts::n(&t.counts.installs) == 0);
    t.hl.find_label(&mut t.app, "At next start");
    t.click("Install now");
    t.settle(|t| matches!(t.app.update.phase, Phase::Installed(_)));
    // spec: UPD-010
    assert!(t.shows("Updated to Shale 9.9.9. Restart to use it."), "{:?}", t.painted());
    // spec: UPD-019
    let out = t.click("Restart now");
    assert!(t.app.update.restart.is_some(), "what to start is remembered");
    assert!(out.viewport_output.values().any(|v| v.commands.contains(&ViewportCommand::Close)), "the window is asked to close, which saves like quitting");
}

// spec: UPD-009
#[test]
fn upd_009_at_next_start_is_saved_in_the_settings() {
    let mut t = T::new(UpdateMode::Download, Some("9.9.9"), portable_kind());
    t.settle(|t| matches!(t.app.update.phase, Phase::Ready(_)));
    t.click("At next start");
    assert_eq!(t.app.cfg.updates.install_on_start, "9.9.9");
    assert!(!t.shows("ready to install"));
}

// spec: UPD-010
#[test]
fn upd_010_install_mode_ends_with_the_restart_notice_and_later_keeps_the_window_open() {
    let mut t = T::new(UpdateMode::Install, Some("9.9.9"), portable_kind());
    t.settle(|t| matches!(t.app.update.phase, Phase::Installed(_)));
    assert!(t.shows("Updated to Shale 9.9.9"));
    let out = t.click("Later");
    assert!(!t.shows("Updated to Shale 9.9.9") && t.app.update.restart.is_none());
    assert!(!out.viewport_output.values().any(|v| v.commands.contains(&ViewportCommand::Close)));
}

// spec: UPD-010
#[test]
fn upd_010_a_failed_update_shows_the_error_with_try_again() {
    let mut t = T::prepared(UpdateMode::Install, Some("9.9.9"), portable_kind(), |_, c| *c.fetch_error.lock().unwrap() = Some("the download of x failed: curl: (22) 404".into()));
    t.settle(|t| matches!(t.app.update.phase, Phase::Failed(_)));
    assert!(t.shows("The update to Shale 9.9.9 failed: the download of x failed"), "{:?}", t.painted());
    *t.counts.fetch_error.lock().unwrap() = None;
    t.click("Try again");
    t.settle(|t| matches!(t.app.update.phase, Phase::Installed(_)));
}

// spec: UPD-012
#[test]
fn upd_012_a_package_managed_install_shows_the_package_manager_command_and_no_install_button() {
    let mut t = T::new(UpdateMode::Install, Some("9.9.9"), InstallKind::PackageManaged);
    t.settle(|t| t.app.update.found.is_some());
    assert!(t.shows("yay -Syu shale-bin"), "{:?}", t.painted());
    assert!(t.hl.try_label(&mut t.app, "Install").is_none() && t.hl.try_label(&mut t.app, "Download").is_none());
    assert_eq!(Counts::n(&t.counts.fetches), 0);
}

// spec: UPD-015
#[test]
fn upd_015_without_write_access_the_notice_gives_the_command_to_copy() {
    let kind = InstallKind::LinuxInstalled { prefix: "/usr/local".into(), user_default: false };
    let mut t = T::prepared(UpdateMode::Install, Some("9.9.9"), kind, |app, _| app.update.writable = false);
    t.settle(|t| matches!(t.app.update.phase, Phase::Ready(_)));
    assert!(t.shows("cannot be written") && t.shows("sudo sh"), "{:?}", t.painted());
    t.click("Copy the command");
    t.click("Dismiss");
    assert!(!t.shows("sudo sh"));
}

// spec: UPD-002
#[test]
fn upd_002_off_shows_no_notice_and_no_question_with_a_newer_release_waiting() {
    let mut t = T::new(UpdateMode::Off, Some("9.9.9"), portable_kind());
    t.hl.frames(&mut t.app, 30);
    assert!(!t.shows("9.9.9") && !t.shows("What should Shale do"));
    assert_eq!(Counts::n(&t.counts.checks), 0);
}

// spec: UPD-020
#[test]
fn upd_020_settings_updates_shows_the_modes_the_version_the_install_and_checks_on_request() {
    let mut t = T::new(UpdateMode::Notify, None, portable_kind());
    t.app.open_settings(SettingsTab::Updates);
    t.hl.frames(&mut t.app, 4);
    assert!(SettingsTab::ALL.contains(&SettingsTab::Updates) && SettingsTab::Updates.label() == "Updates");
    for text in ["When an update is found", "Ask me when an update is found", "This copy of Shale", "Version 0.6.6", "Installed as: Linux, run from the unpacked download in /p", "It can update itself.", "Check for updates now"] {
        assert!(t.shows(text), "{text}: {:?}", t.painted());
    }
    for m in UpdateMode::CHOICES {
        assert!(t.shows(m.label()) && t.shows(m.about()), "{m:?}");
    }
    // the result of a check, in words
    t.settle(|t| t.app.update.result.is_some());
    assert!(t.shows("You have the latest version, 0.6.6."), "{:?}", t.painted());
    assert!(t.shows("Last check:"), "{:?}", t.painted());
    let before = Counts::n(&t.counts.checks);
    t.click("Check for updates now");
    t.settle(|t| Counts::n(&t.counts.checks) > before && !t.app.update.busy());
    // a found release and a failing check
    *t.counts.latest.lock().unwrap() = Some(super::logic_tests::release_for_tests("9.9.9"));
    t.click("Check for updates now");
    t.settle(|t| matches!(t.app.update.result, Some(CheckResult::Available(_))));
    assert!(t.shows("Shale 9.9.9 is available."));
    *t.counts.check_error.lock().unwrap() = Some("could not reach GitHub".into());
    t.click("Check for updates now");
    t.settle(|t| matches!(t.app.update.result, Some(CheckResult::Failed(_))));
    assert!(t.shows("The check failed: could not reach GitHub"), "{:?}", t.painted());
}

// spec: UPD-020
#[test]
fn upd_020_choosing_a_mode_in_the_settings_is_saved_and_off_disables_the_check_button() {
    let mut t = T::new(UpdateMode::Notify, None, portable_kind());
    t.app.open_settings(SettingsTab::Updates);
    t.hl.frames(&mut t.app, 4);
    t.click("Install");
    assert_eq!(t.app.cfg.updates.mode, UpdateMode::Install);
    t.click("Off");
    assert_eq!(t.app.cfg.updates.mode, UpdateMode::Off);
    assert!(t.shows("Off: no request is made and nothing is shown."), "{:?}", t.painted());
    let before = Counts::n(&t.counts.checks);
    t.click("Check for updates now");
    t.hl.frames(&mut t.app, 10);
    assert_eq!(Counts::n(&t.counts.checks), before, "the disabled button does not ask the network");
}

// spec: UPD-020
#[test]
fn upd_020_settings_says_why_a_package_managed_install_cannot_update_itself_and_when_the_environment_is_off() {
    let mut t = T::new(UpdateMode::Notify, None, InstallKind::PackageManaged);
    t.app.update.env_off = true;
    t.app.open_settings(SettingsTab::Updates);
    t.hl.frames(&mut t.app, 4);
    assert!(t.shows("Installed as: Linux, installed by a package manager"), "{:?}", t.painted());
    assert!(t.shows("yay -Syu shale-bin"));
    assert!(t.shows("SHALE_UPDATES=off is set in the environment"), "{:?}", t.painted());
    let mut t = T::new(UpdateMode::Notify, None, InstallKind::LinuxPortable { dir: "/opt/x".into(), whole: true });
    t.app.update.writable = false;
    t.app.open_settings(SettingsTab::Updates);
    t.hl.frames(&mut t.app, 4);
    assert!(t.shows("/opt/x cannot be written by you") && t.shows("you install them by hand"), "{:?}", t.painted());
}

/// A window whose check, download and install wait for the returned gate (the check starts in the first frame).
fn held(mode: UpdateMode, latest: Option<&str>) -> (T, Arc<AtomicBool>) {
    let (backend, counts, gate) = gated(latest);
    let mut t = T::prepared(mode, None, portable_kind(), |app, _| app.update.backend = backend);
    t.counts = counts;
    (t, gate)
}

// spec: JOB-036
#[test]
fn job_036_the_check_is_a_quiet_task_that_can_stop_and_the_settings_page_shows_it() {
    let (mut t, gate) = held(UpdateMode::Notify, Some("9.9.9"));
    t.app.open_settings(SettingsTab::Updates);
    let task = t.app.busy.info(&UPDATE_CHECK).expect("the check is a task");
    assert_eq!((task.label.as_str(), task.cancellable), ("Checking for updates…", true));
    assert!(t.app.busy.list().is_empty(), "a check that also runs by itself is quiet in the indicator");
    assert!(t.shows("Checking for updates…") && t.shows("Cancel"), "{:?}", t.painted());
    // Cancel ends it without a word of a failure, and the button is back
    t.click("Cancel");
    t.settle(|t| !t.app.update.busy());
    assert!(t.app.busy.list().is_empty() && t.app.update.result.is_none(), "{:?}", t.app.update.result);
    assert!(t.shows("Check for updates now") && !t.shows("The check failed"), "{:?}", t.painted());
    // a check that is asked for runs to its end
    t.click("Check for updates now");
    assert!(t.app.busy.is_running(&UPDATE_CHECK), "the button started the check");
    assert!(t.hl.try_label(&mut t.app, "Check for updates now").is_some());
    gate.store(true, std::sync::atomic::Ordering::SeqCst);
    t.settle(|t| matches!(t.app.update.result, Some(CheckResult::Available(_))));
    assert!(t.app.busy.list().is_empty());
}

// spec: JOB-036
#[test]
fn job_036_the_download_is_a_task_that_can_stop_and_the_notice_shows_it_with_cancel() {
    let (mut t, gate) = held(UpdateMode::Notify, Some("9.9.9"));
    gate.store(true, std::sync::atomic::Ordering::SeqCst);
    t.settle(|t| t.app.update.found.is_some());
    gate.store(false, std::sync::atomic::Ordering::SeqCst);
    t.click("Download");
    let task = t.app.busy.info(&UPDATE_DOWNLOAD).expect("the download is a task");
    assert_eq!((task.label.as_str(), task.cancellable), ("Downloading Shale 9.9.9…", true));
    assert_eq!(t.app.busy.list().len(), 1, "and the indicator lists it");
    assert!(t.shows("Downloading Shale 9.9.9…") && t.shows("Cancel"), "{:?}", t.painted());
    // Cancel: the notice is back with its buttons, and nothing is left running
    t.click("Cancel");
    t.settle(|t| t.app.update.phase == Phase::Idle && !t.app.update.busy());
    assert!(t.shows("Shale 9.9.9 is available.") && t.hl.try_label(&mut t.app, "Download").is_some(), "{:?}", t.painted());
    assert!(t.app.busy.list().is_empty());
    // the same download, not stopped
    t.click("Download");
    gate.store(true, std::sync::atomic::Ordering::SeqCst);
    t.settle(|t| matches!(t.app.update.phase, Phase::Ready(_)));
    assert!(t.app.busy.list().is_empty());
}

// spec: JOB-036
#[test]
fn job_036_the_install_is_a_task_that_cannot_stop_and_the_notice_says_what_it_does() {
    let (mut t, gate) = held(UpdateMode::Notify, Some("9.9.9"));
    gate.store(true, std::sync::atomic::Ordering::SeqCst);
    t.settle(|t| t.app.update.found.is_some());
    t.click("Download");
    t.settle(|t| matches!(t.app.update.phase, Phase::Ready(_)));
    gate.store(false, std::sync::atomic::Ordering::SeqCst);
    t.click("Install now");
    let task = t.app.busy.info(&UPDATE_INSTALL).expect("the install is a task");
    assert_eq!((task.label.as_str(), task.cancellable), ("Installing Shale 9.9.9…", false));
    assert!(t.shows("Installing Shale 9.9.9…") && !t.shows("Cancel"), "{:?}", t.painted());
    gate.store(true, std::sync::atomic::Ordering::SeqCst);
    t.settle(|t| matches!(t.app.update.phase, Phase::Installed(_)));
    assert!(t.app.busy.list().is_empty());
}

// spec: JOB-036
#[test]
fn job_036_a_step_that_finds_its_task_still_listed_keeps_what_it_had_and_tries_again() {
    let (mut t, gate) = held(UpdateMode::Notify, Some("9.9.9"));
    gate.store(true, std::sync::atomic::Ordering::SeqCst);
    t.settle(|t| t.app.update.found.is_some());
    let ctx = egui::Context::default();
    let mut cfg = UpdateConfig::default();
    // a download while the task of a download is listed: nothing starts, the notice stays
    let held_download = t.app.busy.begin(UPDATE_DOWNLOAD, "held by the test").unwrap();
    t.app.update.act(&ctx, &mut cfg, Action::Download);
    assert_eq!((t.app.update.phase.clone(), t.app.update.busy()), (Phase::Idle, false));
    drop(held_download);
    t.click("Download");
    t.settle(|t| matches!(t.app.update.phase, Phase::Ready(_)));
    // an install while the task of an install is listed: the downloaded file stays ready
    let held_install = t.app.busy.begin(UPDATE_INSTALL, "held by the test").unwrap();
    t.app.update.act(&ctx, &mut cfg, Action::Install);
    assert!(matches!(t.app.update.phase, Phase::Ready(_)) && !t.app.update.busy(), "{:?}", t.app.update.phase);
    drop(held_install);
    // a check that is asked for while the task of a check is listed waits, and the request is not lost
    let held_check = t.app.busy.begin(UPDATE_CHECK, "held by the test").unwrap();
    t.app.update.check_now();
    t.app.update.tick(&ctx, &mut cfg);
    assert!(t.app.update.force && !t.app.update.busy());
    drop(held_check);
    t.settle(|t| !t.app.update.force);
}
