//! "Open user guide" (DEV-020): the guide is copied into a folder of its own, per version, and
//! opened as a vault like any other. What finds and copies it is `crate::userguide`.

use super::ShaleApp;

impl ShaleApp {
    /// Open the user guide as the vault (F1, Settings → Help). Says in words why when it cannot.
    pub fn open_user_guide(&mut self) {
        match crate::userguide::prepare(&self.guide, env!("CARGO_PKG_VERSION")) {
            Ok(dir) => {
                // the guide opens on its start note (tabs the person left in an older visit stay)
                self.open_vault_in_background(&dir, super::vault_load::After::Note("Start here.md".into()));
            }
            Err(e) => self.toast(e),
        }
    }
}
