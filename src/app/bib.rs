//! The window's side of the bibliography (EDIT-123, JOB-039): which `.bib` files the open note and
//! the vault's default ask for, the step that reads them in the background, and the bibliography in
//! effect for the open note. The editor's use of it is #646.

use super::{ShaleApp, View};
use crate::bibliography::resolve::{self, Wanted};
use crate::bibliography::Effect;
use eframe::egui;
use std::path::{Path, PathBuf};

/// The files the window asks for now.
pub(crate) struct Needs {
    /// the vault's default (Settings), also when the open note names its own
    pub default: Wanted,
    /// the open note's path and its files (its property, else the default)
    pub note: Option<(PathBuf, Wanted)>,
}

impl ShaleApp {
    /// What the vault's default and the note in the focused pane ask for; `None` without a vault.
    pub(crate) fn bib_needs(&self) -> Option<Needs> {
        let vault = self.vault.as_ref()?;
        let default = self.vdata.settings.bibliography.as_str();
        let note = match self.current_tab().map(|t| &t.view) {
            Some(View::Note(rel)) => self.notes.get(rel).map(|d| (rel.as_path(), d.text.as_str())),
            _ => None,
        };
        Some(Needs { default: resolve::wanted(vault, None, default), note: note.map(|(rel, text)| (rel.to_path_buf(), resolve::wanted(vault, Some((rel, text)), default))) })
    }

    /// Read what is asked for and has changed since it was read (a task of the busy registry).
    pub fn bib_tick(&mut self, ctx: &egui::Context) {
        let Some(needs) = self.bib_needs() else { return };
        let files: Vec<&Path> = needs.default.files.iter().chain(needs.note.iter().flat_map(|(_, w)| w.files.iter())).map(PathBuf::as_path).collect();
        let Some(vault) = self.vault.as_ref() else { return };
        let ctx = ctx.clone();
        self.bib.tick(vault, &self.busy, &files, &move || ctx.request_repaint());
    }

    /// The bibliography in effect for the vault's default.
    pub(crate) fn bib_default_effect(&self) -> Effect {
        self.bib_needs().map(|n| self.bib.effect(&n.default)).unwrap_or_default()
    }

    /// The open note's name and text, and the bibliography in effect for it.
    pub(crate) fn bib_note_effect(&self) -> Option<(String, String, Effect)> {
        let (rel, wanted) = self.bib_needs()?.note?;
        let text = self.notes.get(&rel)?.text.clone();
        Some((rel.file_name()?.to_string_lossy().into_owned(), text, self.bib.effect(&wanted)))
    }
}
