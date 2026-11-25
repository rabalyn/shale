//! The thesis document for the exports that run without the window (ADR-0039, #648): the built-in
//! PDF and the site. They read the bibliography files themselves, once each, and make the same
//! [`View`] the reading view has, so the numbers, the citations and the bibliography are the same.

use super::{book, Book, View};
use crate::bibliography::format::Style;
use crate::bibliography::reader::{self, FileIndex};
use crate::bibliography::{resolve, Effect};
use crate::markdown::labels::Source;
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
        self.effect_for(vault, Some((rel, text)))
    }

    /// The bibliography in effect for a note, or for a book without a top note: the vault's default.
    fn effect_for(&mut self, vault: &Vault, note: Option<(&Path, &str)>) -> Effect {
        let wanted = resolve::wanted(vault, note, &self.default);
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
    pub fn view(&mut self, vault: &Vault, rel: &Path, text: &str, style: Option<Style>) -> Option<Arc<View>> {
        if !text.contains('@') && !text.contains("{#") {
            return None;
        }
        let effect = self.effect(vault, rel, text);
        Some(Arc::new(View::build_styled(None, &[text], 0, text, effect, true, style)))
    }

    /// The views of the notes of a book: `top` is the top folder note (its path, its text and its
    /// index among the `sources` when it is printed), whose `bibliography:`, `citation-style:`,
    /// `float-numbering:`, `language:` and caption positions are the book's; a chapter's own are
    /// ignored. `None` for a book none of whose notes has an `@` or a `{#`.
    pub fn book(&mut self, vault: &Vault, book: Book, sources: &[Source], top: Option<(&Path, &str, Option<usize>)>, style: Option<Style>) -> Option<BookViews> {
        if !sources.iter().any(|s| s.text.contains('@') || s.text.contains("{#")) {
            return None;
        }
        let effect = self.effect_for(vault, top.map(|(rel, text, _)| (rel, text)));
        let views = View::of_book(Arc::new(book), sources, top.map_or("", |(_, t, _)| t), effect, style);
        let notes: Vec<book::Note> = sources.iter().enumerate().map(|(i, s)| book::Note { name: s.title.unwrap_or("?"), text: s.text, top: top.is_some_and(|(_, _, at)| at == Some(i)) }).collect();
        let warnings = views.first().map(|v| book::warnings(&notes, &v.labels, &v.bib)).unwrap_or_default();
        Some(BookViews { views: views.into_iter().map(Arc::new).collect(), warnings })
    }
}

/// The thesis document of a book for an export that lays out all its notes (BOOK-020, BOOK-021).
pub struct BookViews {
    /// one view per note, in reading order: the same numbers, bibliography and citations
    pub views: Vec<Arc<View>>,
    /// what the export has to say about the thesis syntax of the book, naming the notes
    pub warnings: Vec<String>,
}
