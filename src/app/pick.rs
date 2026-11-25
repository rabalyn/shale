//! The window's side of the file picker (PICK-001): each frame the host answers the asks of the
//! parts of the window, and the vault dropdown takes the folder it asked for.

use super::ShaleApp;
use crate::config::Config;
use crate::file_picker::places::Inputs;
use crate::known_vaults::canonical;
use crate::vault::Vault;
use eframe::egui::Context;
use std::path::PathBuf;

impl ShaleApp {
    /// One frame of the picker: the system's dialog or Shale's own, and what a vault dialog asked for.
    pub(crate) fn pick_tick(&mut self, ctx: &Context) {
        let (cfg, vault) = (&self.cfg, self.vault.as_ref());
        self.pick.frame(ctx, cfg.system_file_dialog, || place_inputs(cfg, vault));
        self.vault_pick_tick();
    }
}

/// What the side shortcuts of Shale's own picker are made from: the account's folders, the known
/// vaults (the open one first) and the library folders (PICK-006).
fn place_inputs(cfg: &Config, vault: Option<&Vault>) -> Inputs {
    let open = vault.map(|v| canonical(&v.root));
    let mut vaults: Vec<(String, PathBuf)> = cfg.vaults.entries().iter().map(|e| (cfg.vaults.label(e), e.path.clone())).collect();
    if let Some(open) = open {
        vaults.sort_by_key(|(_, p)| *p != open);
    }
    let libraries = cfg.media_libraries.iter().map(|l| (l.name.clone(), l.path.clone())).collect();
    Inputs::of_this_computer(vaults, libraries)
}
