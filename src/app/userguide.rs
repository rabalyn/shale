//! "Open user guide" (DEV-020): the guide is copied into a folder of its own, per version, and
//! opened as a vault like any other. What finds and copies it is `crate::userguide`.

use super::ShaleApp;

impl ShaleApp {
    /// Open the user guide as the vault (F1, Settings → Help). Says in words why when it cannot.
    pub fn open_user_guide(&mut self) {
        self.open_guide_note("Start here", None);
    }

    /// Open the user guide at a heading of a note, as a link of the app's own help (SYNC-082).
    pub fn open_user_guide_at(&mut self, note: &str, heading: &str) {
        self.open_guide_note(note, Some(heading.to_string()));
    }

    fn open_guide_note(&mut self, note: &str, heading: Option<String>) {
        match crate::userguide::prepare(&self.guide, env!("CARGO_PKG_VERSION")) {
            Ok(dir) => {
                // the guide opens on its start note (tabs the person left in an older visit stay)
                self.open_vault_in_background(&dir, super::vault_load::After::Guide(format!("{note}.md").into(), heading));
            }
            Err(e) => self.toast(e),
        }
    }
}
