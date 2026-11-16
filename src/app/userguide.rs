//! "Open user guide" (DEV-020): the guide is copied into a folder of its own, per version, and
//! opened as a vault like any other. What finds and copies it is `crate::userguide`.

use super::ShaleApp;

impl ShaleApp {
    /// Open the user guide as the vault (F1, Settings → Help). Says in words why when it cannot.
    pub fn open_user_guide(&mut self) {
        match crate::userguide::prepare(&self.guide, env!("CARGO_PKG_VERSION")) {
            Ok(dir) => {
                self.open_vault(&dir);
                // the guide opens on its start note (tabs the person left in an older visit stay)
                let start = std::path::Path::new("Start here.md");
                if self.vault.as_ref().is_some_and(|v| v.file(start).is_some()) {
                    self.open(start, false);
                }
            }
            Err(e) => self.toast(e),
        }
    }
}
