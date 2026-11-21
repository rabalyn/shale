//! The state of the panel and of the comparison view; a sub-state of the app's `HistoryState`.

use super::compare::Loaded;
use super::place::Place;
use crate::gitsync::history::{Deleted, Version};
use eframe::egui;
use std::collections::HashSet;
use std::path::PathBuf;

/// The list of the versions of the note of the focused pane (HIST-004…HIST-007).
#[derive(Default)]
pub struct Panel {
    /// The note the list should show: set by the drawing of the panel, taken by the next frame's read.
    pub want: Option<PathBuf>,
    /// The note `versions` is of.
    pub note: Option<PathBuf>,
    pub versions: Vec<Version>,
    pub loading: bool,
    pub error: Option<String>,
    /// A snapshot or a sync may have changed the list: read it again when it is shown.
    pub stale: bool,
    /// Where the history was drawn last frame (None: nowhere); for tests.
    pub place: Option<Place>,
    /// Where the rows and buttons were drawn, for UI tests.
    pub(crate) rects: Vec<(String, egui::Rect)>,
}

/// What the comparison view shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    /// The note with its changes marked (HIST-009).
    #[default]
    Changes,
    /// The Markdown side by side (HIST-012).
    Source,
}

/// What the view got for the two versions.
pub enum Data {
    Text(Box<super::compare::TextPair>),
    Drawing { before: crate::drawing::Scene, after: crate::drawing::Scene },
    Binary { before: usize, after: usize },
}

impl Data {
    /// Make the data from what the job read; a drawing is parsed here.
    pub fn of(rel: &std::path::Path, loaded: Loaded) -> Data {
        match loaded {
            Loaded::Text(t) => Data::Text(t),
            Loaded::Drawing { before, after } => {
                let scene = |text: &str| crate::drawing::Scene::from_file_text(rel, text).unwrap_or_default();
                Data::Drawing { before: scene(&before), after: scene(&after) }
            }
            Loaded::Binary { before, after } => Data::Binary { before, after },
        }
    }
}

/// The comparison view (HIST-008…HIST-015).
#[derive(Default)]
pub struct Changes {
    /// The note (or drawing) compared.
    pub rel: PathBuf,
    /// The older end: a version's commit.
    pub from: String,
    /// The newer end: a version's commit, or None for the note as it is now.
    pub to: Option<String>,
    pub mode: Mode,
    pub data: Option<Data>,
    pub loading: bool,
    pub error: Option<String>,
    /// The folds that are opened, by the first row they hide (HIST-010).
    pub open_folds: HashSet<usize>,
    /// The change the view is at (an index of `change_starts`), and that the view should scroll to it.
    pub current: usize,
    pub jump: bool,
    /// The slider's value while it is dragged: the version is read when it is let go (HIST-014).
    pub scrub: Option<usize>,
    /// One version's text shown alone, read-only: its title and the text (HIST-014).
    pub alone: Option<(String, String)>,
    /// Where the buttons were drawn, for UI tests.
    pub(crate) rects: Vec<(String, egui::Rect)>,
}

impl Changes {
    /// Where the changes of a text begin (rows).
    pub fn starts(&self) -> Vec<usize> {
        match &self.data {
            Some(Data::Text(t)) => super::diff::change_starts(&t.rows),
            _ => vec![],
        }
    }
}

/// The deleted notes (SYNC-040), until they are folded into the panel.
#[derive(Default)]
pub struct DeletedList {
    pub list: Vec<Deleted>,
    pub loading: bool,
    pub error: Option<String>,
    pub(crate) rects: Vec<(String, egui::Rect)>,
}
