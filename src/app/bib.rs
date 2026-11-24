//! The window's side of the bibliography (EDIT-123, JOB-039): which `.bib` files the open note and
//! the vault's default ask for, the step that reads them in the background, and the bibliography in
//! effect for the open note. The editor and the reading view take the document of the open note
//! (its numbers, the bibliography, the formatted citations) from [`ShaleApp::thesis_view`] (#646).

use super::{ShaleApp, View};
use crate::bibliography::resolve::{self, Wanted};
use crate::bibliography::Effect;
use crate::thesis::{Book, View as Thesis};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The files the window asks for now.
pub(crate) struct Needs {
    /// the vault's default (Settings), also when the open note names its own
    pub default: Wanted,
    /// the open note's path and its files (its property, else the default)
    pub note: Option<(PathBuf, Wanted)>,
}

/// What the window keeps of the thesis document of the open note (#646).
#[derive(Default)]
pub(crate) struct ThesisState {
    /// the book of the note it was last asked about, and what that was worked out for
    book: Mutex<Option<(BookKey, Option<Arc<Book>>)>>,
    /// the last view and what it was made for
    view: Option<(ViewKey, Arc<Thesis>)>,
}

/// What the book of a note depends on: the note, the size of the vault's file tree and the
/// properties of the note itself (it may be the top folder note, which is edited).
#[derive(PartialEq)]
struct BookKey(PathBuf, usize, usize, u64);

/// What a [`Thesis`] view is made for: the note, its text, the vault's state, the bibliography in
/// effect and whether the citations are formatted.
#[derive(PartialEq)]
struct ViewKey {
    rel: PathBuf,
    text: u64,
    generation: u64,
    bib: u64,
    format: bool,
}

/// The files, the problems and the read state of a bibliography in effect, as a number: it changes when a file was read again.
fn fingerprint(effect: &Effect) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (rel, index) in &effect.files {
        rel.hash(&mut h);
        Arc::as_ptr(index).hash(&mut h);
    }
    (effect.loading, &effect.messages).hash(&mut h);
    h.finish()
}

impl ShaleApp {
    /// The thesis book that `rel` is a note of (ADR-0039 §5: one bibliography per book), if any.
    pub(crate) fn thesis_book(&self, rel: &Path) -> Option<Arc<Book>> {
        let vault = self.vault.as_ref()?;
        let own = self.notes.get(rel).map_or(0, |d| crate::fold_ui::hash_of(&format!("{:?}", crate::markdown::front_matter_values(&d.text))));
        let key = BookKey(rel.to_path_buf(), vault.files.len(), vault.folders.len(), own);
        let mut cache = self.thesis.book.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, book)) = cache.as_ref().filter(|(k, _)| *k == key) {
            return book.clone();
        }
        let text = |p: &Path| self.notes.get(p).map(|d| d.text.clone()).or_else(|| vault.notes.get(p).map(|n| n.text.clone())).unwrap_or_default();
        let book = Book::of(vault, rel, &text).map(Arc::new);
        *cache = Some((key, book.clone()));
        book
    }

    /// What the vault's default and the note in the focused pane ask for; `None` without a vault.
    pub(crate) fn bib_needs(&self) -> Option<Needs> {
        let vault = self.vault.as_ref()?;
        let default = self.vdata.settings.bibliography.as_str();
        let note = match self.current_tab().map(|t| &t.view) {
            Some(View::Note(rel)) => self.notes.get(rel).map(|d| (rel.as_path(), d.text.as_str())),
            _ => None,
        };
        // in a book only the top folder note's property counts (ADR-0039 §5)
        let wanted = |rel: &Path, text: &str| match self.thesis_book(rel) {
            Some(book) => {
                let top = self.notes.get(&book.top).map(|d| d.text.clone()).or_else(|| vault.notes.get(&book.top).map(|n| n.text.clone())).unwrap_or_default();
                resolve::wanted(vault, Some((&book.top, &top)), default)
            }
            None => resolve::wanted(vault, Some((rel, text)), default),
        };
        Some(Needs { default: resolve::wanted(vault, None, default), note: note.map(|(rel, text)| (rel.to_path_buf(), wanted(rel, text))) })
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

impl ShaleApp {
    /// The thesis document of the note `rel` (its numbers, bibliography and, with `format`, its
    /// formatted citations), made again when the text, the vault or the bibliography changed.
    pub(crate) fn thesis_view(&mut self, rel: &Path, format: bool) -> Arc<Thesis> {
        let Some(vault) = self.vault.as_ref() else { return Arc::default() };
        let Some(doc) = self.notes.get(rel) else { return Arc::default() };
        let effect = match self.bib_needs().and_then(|n| n.note) {
            Some((r, wanted)) if r == rel => self.bib.effect(&wanted),
            _ => Effect::default(),
        };
        let key = ViewKey { rel: rel.to_path_buf(), text: crate::fold_ui::hash_of(&doc.text), generation: vault.generation, bib: fingerprint(&effect), format };
        if let Some((_, view)) = self.thesis.view.as_ref().filter(|(k, _)| *k == key) {
            return view.clone();
        }
        let book = self.thesis_book(rel);
        // a note with no `@` and no label is nothing the thesis rules read, and it stands alone: no work
        let view = match &book {
            None if !doc.text.contains('@') && !doc.text.contains("{#") => Thesis { bib: effect, ..Thesis::default() },
            None => Thesis::build(None, &[&doc.text], 0, &doc.text, effect, format),
            Some(book) => {
                let text_of = |p: &Path| -> &str {
                    if p == rel {
                        &doc.text
                    } else {
                        self.notes.get(p).map(|d| d.text.as_str()).or_else(|| vault.notes.get(p).map(|n| n.text.as_str())).unwrap_or("")
                    }
                };
                let texts: Vec<&str> = book.notes.iter().map(|(p, _)| text_of(p)).collect();
                let index = book.notes.iter().position(|(p, _)| p == rel).unwrap_or(0);
                Thesis::build(Some(book.clone()), &texts, index, text_of(&book.top), effect, format)
            }
        };
        let view = Arc::new(view);
        self.thesis.view = Some((key, view.clone()));
        view
    }
}
