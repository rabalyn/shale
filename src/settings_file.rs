//! The settings file as a tab of the window (CFG-011 to CFG-014): the text of
//! `~/.config/shale/config.toml` is edited like a note, but the file is not part of any vault, so
//! it makes no note, is not in the file tree and is not in the lists of open or recent files.
//! Saving it applies the settings; a file changed by another program is picked up.

use crate::app::{ShaleApp, View};
use crate::config::{self, Config};
use crate::theme;
use crate::vault::{FileStamp, Stamp};
use eframe::egui::{self, Ui};
use std::time::{Duration, Instant};

/// How long the text of the tab rests before it is saved by itself.
const AUTOSAVE_AFTER: Duration = Duration::from_millis(800);

/// How often the file is looked at for a change made by another program.
const WATCH_EVERY: Duration = Duration::from_secs(1);

/// The text of the settings file in the tab, and what is known about the file on disk.
pub struct SettingsFile {
    /// What the editor shows.
    pub text: String,
    /// The text has changes that are not in the file.
    pub dirty: bool,
    pub(crate) last_edit: Instant,
    /// The content the file had when Shale last read or wrote it: another content is a change
    /// made by another program.
    base: Option<Stamp>,
    /// The file as the last look found it (cheap to compare, see `FileStamp`).
    seen: Option<FileStamp>,
    pub(crate) checked: Instant,
    /// Why `text` is not valid settings, or why the file on disk is not.
    pub error: Option<String>,
    /// The file changed on disk while the tab had text of its own that is not saved.
    pub disk_changed: bool,
    /// The last problem that was shown as a message, so it is said once.
    said: Option<String>,
}

impl SettingsFile {
    pub fn new() -> SettingsFile {
        SettingsFile { text: String::new(), dirty: false, last_edit: Instant::now(), base: None, seen: None, checked: Instant::now(), error: None, disk_changed: false, said: None }
    }

    /// Take the file as it is on disk now, `fallback` when there is none.
    fn load(&mut self, path: &std::path::Path, fallback: impl FnOnce() -> String) {
        let bytes = crate::fsutil::read(path).ok();
        self.base = bytes.as_deref().map(Stamp::of_bytes);
        self.seen = FileStamp::of(path);
        self.text = bytes.map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_else(fallback);
        self.dirty = false;
        self.disk_changed = false;
        self.error = Config::parse_text(&self.text).err();
    }

    /// Only note what is on disk (the tab does not exist yet, or shows something else).
    fn mark(&mut self, path: &std::path::Path) {
        self.base = crate::fsutil::read(path).ok().as_deref().map(Stamp::of_bytes);
        self.seen = FileStamp::of(path);
    }
}

impl ShaleApp {
    /// The first look at the file at the start: what is there now is not a change (CFG-013).
    pub(crate) fn settings_file_baseline(&mut self) {
        if let (true, Some(p)) = (self.persist_config, config::config_path()) {
            self.sf.mark(&p);
        }
    }

    /// Open the settings file in a tab (CFG-011). A text typed there and not saved yet stays.
    pub fn open_settings_file(&mut self) {
        let Some(path) = config::config_path().filter(|_| self.persist_config) else {
            self.toast("Settings are not kept in this mode, so there is no settings file");
            return;
        };
        self.save_config();
        if !self.sf.dirty {
            self.sf.load(&path, || self.cfg.file_text());
        }
        self.show_view(View::SettingsFile, false);
    }

    /// Put `new` in place of the settings in use, and everything that follows from them.
    fn apply_config(&mut self, mut new: Config) {
        new.load_secrets();
        crate::data_usage::dialog::sanitize(&mut new.caches);
        if self.keymap.overrides() != new.keybindings {
            self.keymap = crate::keys::Keymap::from_overrides(&new.keybindings);
        }
        let log_was_on = self.cfg.input_log;
        self.cfg = new;
        if self.cfg.input_log && !log_was_on {
            self.start_input_log();
        }
    }

    /// Save the text of the tab as the settings file and use it (CFG-011). Text that is not
    /// valid settings is not written: the file stays as it was, the settings in use stay, the
    /// text stays in the tab and the problem is said (CFG-014). `manual`: someone asked, so
    /// the problem is always shown as a message.
    pub fn save_settings_file(&mut self, manual: bool) -> bool {
        let Some(path) = config::config_path() else { return false };
        let (mut new, newer) = match Config::parse_text(&self.sf.text) {
            Ok(ok) => ok,
            Err(e) => {
                let m = format!("The settings were not saved, the file has an error: {e}");
                if manual || self.sf.said.as_deref() != Some(m.as_str()) {
                    self.toast(m.clone());
                }
                self.sf.said = Some(m);
                self.sf.error = Some(e);
                return false;
            }
        };
        if let Err(e) = config::write_private(&path, self.sf.text.as_bytes()) {
            self.toast(format!("Could not save the settings file: {e}"));
            return false;
        }
        self.sf.mark(&path);
        self.sf.dirty = false;
        self.sf.disk_changed = false;
        self.sf.error = None;
        self.sf.said = None;
        new.read_only = newer;
        self.apply_config(new);
        true
    }

    /// Shale has written the settings itself (`save_config`): that is no change by another
    /// program, and a tab without text of its own shows the new file.
    pub(crate) fn settings_file_written(&mut self) {
        let Some(path) = config::config_path() else { return };
        if self.sf.dirty {
            let was = self.sf.base;
            self.sf.mark(&path);
            self.sf.disk_changed |= was != self.sf.base;
        } else {
            self.sf.load(&path, String::new);
        }
    }

    /// Look for a change of the file made by another program (CFG-013). `force`: now, not
    /// only once a second.
    pub fn check_settings_file(&mut self, force: bool) {
        if !self.persist_config || (!force && self.sf.checked.elapsed() < WATCH_EVERY) {
            return;
        }
        self.sf.checked = Instant::now();
        let Some(path) = config::config_path() else { return };
        let now = FileStamp::of(&path);
        if now == self.sf.seen {
            return;
        }
        self.sf.seen = now;
        let Ok(bytes) = crate::fsutil::read(&path) else { return };
        let stamp = Stamp::of_bytes(&bytes);
        if self.sf.base == Some(stamp) {
            return;
        }
        self.sf.base = Some(stamp);
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let parsed = Config::parse_text(&text);
        match &parsed {
            Ok(_) => self.sf.said = None,
            Err(e) => {
                // never written over by Shale until it is valid again (CFG-014)
                self.cfg.read_only = true;
                let m = format!("The settings file {} has an error and is not used until it is fixed: {e}", path.display());
                if self.sf.said.as_deref() != Some(m.as_str()) {
                    self.toast(m.clone());
                }
                self.sf.said = Some(m);
            }
        }
        if let Ok((mut new, newer)) = parsed.clone() {
            new.read_only = newer;
            self.apply_config(new);
        }
        if self.sf.dirty {
            self.sf.disk_changed = true;
            self.toast("The settings file was changed by another program; the open tab keeps its own text until you load the file or save the tab");
        } else {
            self.sf.text = text;
            self.sf.error = parsed.err();
        }
    }

    /// Every frame: the file is looked at, and a text that rests and is valid is saved.
    pub(crate) fn settings_file_tick(&mut self, ctx: &egui::Context) {
        if !self.persist_config {
            return;
        }
        self.check_settings_file(false);
        self.settings_file_flush(false);
        ctx.request_repaint_after(WATCH_EVERY);
    }

    /// Save the text of the tab when it has changes that are valid. `now`: do not wait for it to rest.
    pub(crate) fn settings_file_flush(&mut self, now: bool) {
        let ready = self.sf.dirty && !self.sf.disk_changed && (now || self.sf.last_edit.elapsed() > AUTOSAVE_AFTER);
        if ready && (self.sf.error.is_none() || now) {
            self.save_settings_file(now);
        }
    }

    /// The tab of the settings file.
    pub(crate) fn settings_file_view(&mut self, ui: &mut Ui) {
        let path = config::config_path().map(|p| p.display().to_string()).unwrap_or_default();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(path).color(theme::palette().muted).size(12.0));
            if ui.add_enabled(self.sf.dirty, egui::Button::new("Save")).on_hover_text("Save the file and use the settings (Ctrl+S)").clicked() {
                self.save_settings_file(true);
            }
            ui.label(egui::RichText::new(if self.sf.dirty { "Not saved yet" } else { "Saved" }).color(theme::palette().muted).size(12.0));
        });
        if let Some(e) = &self.sf.error {
            ui.label(egui::RichText::new(format!("This text is not valid settings, so it is not used: {e}")).color(theme::palette().link_missing).size(12.0));
        }
        if self.sf.disk_changed {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("The file was changed by another program. Saving here replaces its text.").color(theme::palette().link_missing).size(12.0));
                if ui.button("Load the file from disk").clicked() {
                    if let Some(p) = config::config_path() {
                        self.sf.load(&p, || self.cfg.file_text());
                    }
                }
            });
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let edit = egui::TextEdit::multiline(&mut self.sf.text).id_salt("settings-file-text").font(egui::TextStyle::Monospace).code_editor().desired_width(f32::INFINITY).desired_rows(30).lock_focus(true);
            if ui.add(edit).changed() {
                self.sf.dirty = true;
                self.sf.last_edit = Instant::now();
                self.sf.error = Config::parse_text(&self.sf.text).err();
            }
        });
    }
}
