//! The bibliography of a note: which `.bib` file(s) it uses, and the index of their keys
//! (EDIT-123, ADR-0039 §5, #645).
//!
//! - [`resolve`]: the property `bibliography:` of a note (or the vault default) to files of the vault.
//! - [`reader`]: a `.bib` file to an index of key, authors, year, title and type; bad entries are
//!   skipped and counted.
//! - [`state`]: the window's sub-state. The files are read in a background job of the busy registry
//!   (JOB-039) and read again when the file's modified time changes.
//! - [`report`]: what Settings shows (the status of the default and of the open note).
//! - [`format`]: citations and the list of references as the five named styles write them (EDIT-127).

pub mod format;
pub mod reader;
pub mod report;
pub mod resolve;
pub mod state;

#[cfg(test)]
mod format_tests;

pub use state::{Bibliographies, Effect};
