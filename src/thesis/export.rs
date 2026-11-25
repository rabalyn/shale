//! The thesis document for the exports that run without the window (ADR-0039, #648): the built-in
//! PDF and the site. They read the bibliography files themselves, once each, and make the same
//! [`View`] the reading view has, so the numbers, the citations and the bibliography are the same.

use super::View;
use crate::bibliography::reader::{self, FileIndex};
use crate::bibliography::{resolve, Effect};
use crate::vault::Vault;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The `.bib` files an export has read so far, and the vault's default bibliography (Settings).
pub struct Bibliographies {
    default: String,
    read: HashMap<PathBuf, Result<Arc<FileIndex>, String>>,
}

impl Bibliographies {
    pub fn new(vault: &Vault) -> Self {
        let default = crate::site_export::rules::read_settings(&vault.root).map(|s| s.bibliography).unwrap_or_default();
        Bibliographies { default, read: HashMap::new() }
    }

    /// The bibliography in effect for the note `rel` with the text `text`.
    fn effect(&mut self, vault: &Vault, rel: &Path, text: &str) -> Effect {
        let wanted = resolve::wanted(vault, Some((rel, text)), &self.default);
        let mut effect = Effect { messages: wanted.problems.clone(), ..Effect::default() };
        for file in &wanted.files {
            match self.read.entry(file.clone()).or_insert_with(|| reader::read_file(&vault.abs(file), None)) {
                Ok(index) => effect.files.push((file.clone(), index.clone())),
                Err(why) => effect.messages.push(format!("The bibliography “{}” {why}. No bibliography is in effect from it.", file.display())),
            }
        }
        effect
    }

    /// The thesis document of a note that stands alone, with its citations formatted; `None` for a
    /// note with no `@` and no `{#`, which has nothing the thesis rules read.
    pub fn view(&mut self, vault: &Vault, rel: &Path, text: &str) -> Option<Arc<View>> {
        if !text.contains('@') && !text.contains("{#") {
            return None;
        }
        let effect = self.effect(vault, rel, text);
        Some(Arc::new(View::build(None, &[text], 0, text, effect, true)))
    }
}
