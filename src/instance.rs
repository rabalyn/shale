//! Only one Shale runs per user: a second one would send a queued mail again, undo the first
//! one's "sending" state and overwrite its settings.

use std::fs::File;
use std::path::Path;

/// Take the lock in `dir`. `Ok(Some(_))`: keep the value until the process ends. `Ok(None)`:
/// another Shale is running.
pub fn acquire(dir: &Path) -> std::io::Result<Option<File>> {
    crate::fsutil::try_lock_file(&dir.join("instance.lock"))
}

/// The folder of the lock: next to the mail store, under the user's data folder.
pub fn default_dir() -> std::path::PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("shale")
}

pub const ALREADY_RUNNING: &str = "Shale is already running. Only one Shale can run at a time, so that mail is never sent twice and settings are not overwritten. Switch to the window that is open.";

/// A tiny window that tells the person, so a double click on the icon is not silent.
pub fn show_already_running() {
    eprintln!("{ALREADY_RUNNING}");
    let options = eframe::NativeOptions { viewport: eframe::egui::ViewportBuilder::default().with_title("Shale").with_app_id("shale").with_inner_size([460.0, 170.0]).with_resizable(false), ..Default::default() };
    // a failure to open the window is not worth more than the message on stderr
    let _ = eframe::run_ui_native("Shale", options, |ui, _frame| {
        ui.add_space(8.0);
        ui.label(ALREADY_RUNNING);
        ui.add_space(12.0);
        if ui.button("Quit").clicked() {
            ui.ctx().send_viewport_cmd(eframe::egui::ViewportCommand::Close);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: DATA-012
    #[test]
    fn a_second_instance_is_refused_until_the_first_has_ended() {
        let dir = std::env::temp_dir().join(format!("shale-instance-test-{}", std::process::id()));
        let first = acquire(&dir).unwrap();
        assert!(first.is_some());
        assert!(acquire(&dir).unwrap().is_none());
        drop(first);
        assert!(acquire(&dir).unwrap().is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
