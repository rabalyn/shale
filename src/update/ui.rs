//! The updates in the window (UPD-007 to UPD-010, UPD-019, UPD-020): the notice with its buttons,
//! the question which mode to use, the release notes, and the page of the settings. What the
//! notice says and offers is [`UpdateState::notice`], a function of the state, so it is tested
//! without a window.

use super::kind::{self, Capability};
use super::{Action, CheckResult, Phase, UpdateConfig, UpdateMode, UpdateState, ENV, UPDATE_CHECK, UPDATE_DOWNLOAD, UPDATE_INSTALL};
use crate::app::ShaleApp;
use crate::busy::{self, Key};
use crate::theme;
use eframe::egui::{self, Align2, Context, CornerRadius, Margin, Stroke, Ui};

/// What a button of the notice does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Press {
    Act(Action),
    /// Put this text (a command) on the clipboard.
    Copy(String),
}

/// What the notice says: a text, and the buttons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    pub error: bool,
    pub buttons: Vec<(&'static str, Press)>,
    /// The task that is running (a download, an install): the window shows its spinner, its text and Cancel where it can stop.
    pub task: Option<Key>,
}

impl UpdateState {
    /// The notice for the state, if there is one (never with updates off, UPD-002).
    pub fn notice(&self, cfg: &UpdateConfig) -> Option<Notice> {
        // while the question which mode to use is open, it is what the window shows
        if self.mode(cfg) == UpdateMode::Off || self.hidden || !self.enabled || self.ask {
            return None;
        }
        let found = self.found.as_ref()?;
        let version = &found.release.version;
        if &cfg.skipped_version == version {
            return None;
        }
        let act = |label: &'static str, a: Action| (label, Press::Act(a));
        let cap = self.capability();
        let notice = |text: String, buttons: Vec<(&'static str, Press)>| Some(Notice { text, error: false, buttons, task: None });
        let running = |text: String, task: Key| Some(Notice { text, error: false, buttons: vec![], task: Some(task) });
        match &self.phase {
            Phase::Idle => {
                let mut text = format!("Shale {version} is available.");
                let mut buttons = vec![act("What's new", Action::ShowNotes)];
                match (&cap, &found.no_file) {
                    (Capability::NotifyOnly(why), _) => text = format!("{text} {why}"),
                    (_, Some(why)) => text = format!("{text} It cannot be downloaded from here: {why}."),
                    (c, None) => {
                        buttons.push(act("Download", Action::Download));
                        if *c == Capability::Install {
                            buttons.push(act("Install", Action::Install));
                        }
                    }
                }
                buttons.push(act("Skip this version", Action::Skip));
                buttons.push(act("Later", Action::Later));
                notice(text, buttons)
            }
            Phase::Downloading { .. } => running(format!("Downloading Shale {version}…"), UPDATE_DOWNLOAD),
            Phase::Ready(staged) => match &cap {
                Capability::Install => {
                    notice(format!("Shale {version} is downloaded and checked, ready to install."), vec![act("Install now", Action::Install), act("At next start", Action::InstallAtStart), act("What's new", Action::ShowNotes)])
                }
                Capability::DownloadOnly(why) | Capability::NotifyOnly(why) => {
                    let command = kind::manual_command(&self.kind, &staged.archive, &staged.dir);
                    notice(format!("Shale {version} is downloaded and checked: {}. {why} To install it, run: {command}", staged.archive.display()), vec![("Copy the command", Press::Copy(command)), act("Dismiss", Action::Dismiss)])
                }
            },
            Phase::Installing => running(format!("Installing Shale {version}…"), UPDATE_INSTALL),
            Phase::Installed(i) => notice(format!("Updated to Shale {}. Restart to use it.", i.version), vec![act("Restart now", Action::RestartNow), act("Later", Action::Later)]),
            Phase::Failed(e) => {
                let retry = if cap == Capability::Install { Action::Install } else { Action::Download };
                Some(Notice { text: format!("The update to Shale {version} failed: {e}"), error: true, buttons: vec![act("Try again", retry), act("Dismiss", Action::Dismiss)], task: None })
            }
        }
    }
}

/// The page of the settings, a line on the result of the last check.
fn result_text(state: &UpdateState, cfg: &UpdateConfig) -> Option<String> {
    let skipped = |v: &str| if cfg.skipped_version == v { " (you chose to skip this version)" } else { "" };
    state.result.as_ref().map(|r| match r {
        CheckResult::UpToDate(v) => format!("You have the latest version, {v}."),
        CheckResult::Available(v) => format!("Shale {v} is available{}.", skipped(v)),
        CheckResult::Failed(e) => format!("The check failed: {e}"),
    })
}

/// Settings → Updates (UPD-020); returns whether a setting changed.
pub fn settings_page(ui: &mut Ui, state: &mut UpdateState, cfg: &mut UpdateConfig) -> bool {
    let mut changed = false;
    theme::hint(ui, "Shale can look for a new release on GitHub (github.com/rabalyn/shale), check its download against the release's SHA-256 file, and install it. A checksum shows the file arrived whole; Shale installs it only when the release also has a signature of the owner of the release key built into Shale.");
    theme::section(ui, "When an update is found");
    crate::settings::anchor::anchor(ui, "updates.mode", |ui| {
        for m in [UpdateMode::Ask, UpdateMode::Off, UpdateMode::Notify, UpdateMode::Download, UpdateMode::Install] {
            if crate::clickable::pointer(ui.radio_value(&mut cfg.mode, m, m.label())).changed() {
                changed = true;
                if m == UpdateMode::Off {
                    state.hidden = true;
                }
            }
            theme::hint(ui, m.about());
        }
    });
    if state.env_off {
        theme::problem(ui, format!("{ENV}=off is set in the environment: updates are off, whatever is chosen here."));
    }
    theme::section(ui, "This copy of Shale");
    ui.label(format!("Version {}", state.current));
    ui.label(format!("Installed as: {}", kind::describe(&state.kind)));
    let how = match state.capability() {
        Capability::Install => "It can update itself.".to_string(),
        Capability::DownloadOnly(why) => format!("{why} Updates are downloaded and checked, and you install them by hand."),
        Capability::NotifyOnly(why) => why,
    };
    theme::hint(ui, how);
    theme::section(ui, "Check");
    let off = state.mode(cfg) == UpdateMode::Off;
    crate::settings::anchor::anchor(ui, "updates.check", |ui| {
        ui.horizontal(|ui| {
            let button = egui::Button::new("Check for updates now");
            let b = if off || state.force {
                ui.add_enabled(false, button).on_disabled_hover_text(if off { "Updates are off: Shale makes no request" } else { "A check is about to start" })
            } else {
                busy::ui::button(ui, &state.busy, &[&UPDATE_CHECK], button)
            };
            if b.clicked() {
                state.check_now();
            }
            busy::ui::status(ui, &state.busy, &UPDATE_CHECK);
        });
    });
    if let Some(t) = result_text(state, cfg).filter(|_| !off) {
        match state.result {
            Some(CheckResult::Failed(_)) => theme::problem(ui, t),
            _ => theme::paragraph(ui, t),
        };
    }
    if cfg.last_check > 0 && !off {
        theme::hint(ui, format!("Last check: {}. Shale checks a short while after it starts and every 24 hours.", super::when(cfg.last_check)));
    }
    if off {
        theme::hint(ui, "Off: no request is made and nothing is shown.");
    }
    changed
}

impl ShaleApp {
    /// One frame of the updates (UPD-003).
    pub(crate) fn update_tick(&mut self, ctx: &Context) {
        if self.update.tick(ctx, &mut self.cfg.updates) {
            self.save_config();
        }
    }

    /// A button of the notice, the question and the notes.
    fn update_press(&mut self, ctx: &Context, press: Press) {
        match press {
            Press::Act(a) => {
                if a == Action::Install || a == Action::Download {
                    self.update.hidden = false;
                }
                if self.update.act(ctx, &mut self.cfg.updates, a) {
                    self.save_config();
                }
            }
            Press::Copy(text) => ctx.copy_text(text),
        }
    }

    /// The notice of the updates in a corner of the window.
    pub(crate) fn paint_update_notice(&mut self, ui: &mut Ui) {
        let Some(notice) = self.update.notice(&self.cfg.updates) else { return };
        let p = theme::palette();
        let (fill, edge) = if notice.error { (p.banner_error_bg, p.link_missing.gamma_multiply(0.7)) } else { (p.banner_notice_bg, p.muted.gamma_multiply(0.7)) };
        let mut pressed = None;
        egui::Area::new(egui::Id::unique("update-notice")).order(egui::Order::Tooltip).anchor(Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -48.0)).show(ui.ctx(), |ui| {
            egui::Frame::new().fill(fill).stroke(Stroke::new(1.0, edge)).corner_radius(CornerRadius::same(8)).inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
                ui.set_max_width(460.0);
                // a download or an install that runs: its spinner and text, and Cancel where it can stop
                let running = notice.task.as_ref().is_some_and(|key| busy::ui::status(ui, &self.busy, key));
                if !running {
                    ui.label(egui::RichText::new(&notice.text).color(p.text));
                }
                ui.horizontal_wrapped(|ui| {
                    for (label, press) in &notice.buttons {
                        if ui.button(*label).clicked() {
                            pressed = Some(press.clone());
                        }
                    }
                });
            });
        });
        if let Some(press) = pressed {
            self.update_press(ui.ctx(), press);
        }
    }

    /// The question which mode to use (UPD-007) and the release notes.
    pub(crate) fn update_dialogs(&mut self, ctx: &Context) {
        if !self.update.enabled {
            return;
        }
        if self.update.ask && self.update.mode(&self.cfg.updates) == UpdateMode::Ask {
            let version = self.update.found.as_ref().map(|f| f.release.version.clone()).unwrap_or_default();
            let mut chosen = None;
            let resp = crate::dialog::Dialog::new("update-ask", "Updates").width(500.0).show(ctx, |ui| {
                theme::paragraph(ui, format!("Shale {version} is available. What should Shale do about updates from now on? You can change it in Settings → Updates."));
                for m in UpdateMode::CHOICES {
                    ui.horizontal(|ui| {
                        if ui.button(m.label()).clicked() {
                            chosen = Some(m);
                        }
                    });
                    theme::hint(ui, m.about());
                }
                theme::hint(ui, "Shale installs a download only when its checksum fits and the owner of its release key signed it.");
            });
            match chosen {
                Some(m) => {
                    self.update.choose(ctx, &mut self.cfg.updates, m);
                    self.save_config();
                }
                // Esc: not now; it asks again at the next check
                None if resp.should_close() => self.update.ask = false,
                None => {}
            }
        }
        if self.update.notes_open {
            if let Some(found) = self.update.found.clone() {
                let mut close = false;
                let resp = crate::dialog::Dialog::new("update-notes", format!("What's new in Shale {}", found.release.version)).width(560.0).show(ctx, |ui| {
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        let notes = if found.release.notes.trim().is_empty() { "The release has no notes." } else { found.release.notes.trim() };
                        theme::paragraph(ui, notes);
                    });
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
                if close || resp.should_close() {
                    self.update.notes_open = false;
                }
            } else {
                self.update.notes_open = false;
            }
        }
    }

    /// Settings → Updates.
    pub(crate) fn updates_tab(&mut self, ui: &mut Ui) -> bool {
        settings_page(ui, &mut self.update, &mut self.cfg.updates)
    }
}
