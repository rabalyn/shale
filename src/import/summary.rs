//! The summary after an import of two or more items (IMP-056): the new notes with a line each,
//! the files that were saved and the errors, with the way to open the notes.

use crate::app::muted;
use crate::theme;
use crate::vault::{rel_to_string, Vault};
use eframe::egui::{self, Ui};
use std::path::{Path, PathBuf};

/// The longest line shown under a note's name.
const LINE_MAX: usize = 110;

pub struct Entry {
    pub rel: PathBuf,
    /// the note's name, without folder and `.md`
    pub name: String,
    /// what the note is about (`line_of`)
    pub line: String,
}

pub struct ImportSummary {
    pub notes: Vec<Entry>,
    /// the files saved as they are (pictures, PDFs, audio, video)
    pub files: Vec<PathBuf>,
    /// the media files that stay where they are (MEDIA-019)
    pub linked: Vec<crate::import::Linked>,
    pub errors: Vec<String>,
    /// "the vault" or "“Folder”": where the notes are
    pub place: String,
}

#[derive(Debug, PartialEq)]
pub enum SummaryAction {
    /// open this note in a tab
    Open(PathBuf),
    /// open all the notes, each in a tab of its own
    OpenAll,
    Close,
}

impl ImportSummary {
    pub fn new(vault: &Vault, notes: &[PathBuf], files: &[PathBuf], linked: &[crate::import::Linked], errors: Vec<String>, place: String) -> Self {
        let notes = notes
            .iter()
            .map(|rel| {
                let name = rel.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let line = vault.notes.get(rel).map(|n| line_of(&n.text)).unwrap_or_default();
                Entry { rel: rel.clone(), name, line }
            })
            .collect();
        ImportSummary { notes, files: files.to_vec(), linked: linked.to_vec(), errors, place }
    }

    /// "Imported 3 notes and 2 files"
    pub fn title(&self) -> String {
        let plural = |n: usize, one: &str, many: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {many}") };
        let mut parts = vec![];
        if !self.notes.is_empty() {
            parts.push(plural(self.notes.len(), "note", "notes"));
        }
        if !self.files.is_empty() {
            parts.push(plural(self.files.len(), "file", "files"));
        }
        if !self.linked.is_empty() {
            parts.push(plural(self.linked.len(), "linked file", "linked files"));
        }
        match parts.split_last() {
            Some((last, first)) if !first.is_empty() => format!("Imported {} and {last}", first.join(", ")),
            _ => format!("Imported {}", parts.join("")),
        }
    }
}

/// What a new note is about, in one line: an e-mail's sender and date, an event's time and place,
/// else the first line of the text that is no heading and no list of links.
pub fn line_of(text: &str) -> String {
    let values = crate::markdown::front_matter_values(text);
    let get = |k: &str| values.iter().find(|(key, _)| key == k).and_then(|(_, v)| v.first()).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let join = |parts: Vec<Option<String>>| parts.into_iter().flatten().collect::<Vec<_>>().join(" · ");
    let line = match get("type").as_deref() {
        Some("email") => join(vec![get("from").map(|f| crate::mail::display_name(&f)), get("date")]),
        Some("event") => join(vec![get("start"), get("location")]),
        _ => String::new(),
    };
    let line = if line.is_empty() { first_text(text) } else { line };
    let one: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > LINE_MAX {
        format!("{}…", one.chars().take(LINE_MAX).collect::<String>().trim_end())
    } else {
        one
    }
}

/// The first line of the note's text after the properties that is plain text: no heading, no
/// list item that is only a link or an embed, no rule.
fn first_text(text: &str) -> String {
    let skip = crate::markdown::front_matter_lines(text);
    text.lines()
        .skip(skip)
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("---") && !(l.starts_with("- ") && (l.contains("[[") || l.starts_with("- **"))) && !l.starts_with("![["))
        .map(|l| l.trim_start_matches("> ").to_string())
        .unwrap_or_default()
}

/// The content of the window; what was clicked.
pub fn show(ui: &mut Ui, s: &ImportSummary) -> Option<SummaryAction> {
    let mut action = None;
    ui.label(egui::RichText::new(s.title()).strong().size(15.0));
    if !s.notes.is_empty() {
        ui.label(muted(format!("The notes are in {}.", s.place)));
    }
    egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
        for e in &s.notes {
            ui.horizontal(|ui| {
                if ui.button("Open").on_hover_text(format!("Open “{}” in a tab", rel_to_string(&e.rel))).clicked() {
                    action = Some(SummaryAction::Open(e.rel.clone()));
                }
                ui.vertical(|ui| {
                    ui.add(egui::Label::new(egui::RichText::new(&e.name).strong()).wrap());
                    if !e.line.is_empty() {
                        ui.add(egui::Label::new(muted(&e.line).size(12.0)).wrap());
                    }
                });
            });
        }
        if !s.files.is_empty() {
            theme::section(ui, "Saved in the media folder");
            for f in &s.files {
                ui.add(egui::Label::new(egui::RichText::new(file_name(f)).size(12.5)).wrap()).on_hover_text(rel_to_string(f));
            }
        }
        if !s.linked.is_empty() {
            theme::section(ui, "Linked where they are");
            for l in &s.linked {
                ui.add(egui::Label::new(egui::RichText::new(file_name(&l.path)).size(12.5)).wrap()).on_hover_text(l.path.display().to_string());
            }
        }
        if !s.errors.is_empty() {
            theme::section(ui, "Went wrong");
            for e in &s.errors {
                ui.add(egui::Label::new(egui::RichText::new(e).color(theme::palette().link_missing).size(12.5)).wrap());
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.add_enabled(!s.notes.is_empty(), egui::Button::new(egui::RichText::new("Open all in tabs").strong())).clicked() {
            action = Some(SummaryAction::OpenAll);
        }
        if ui.button("Close").clicked() {
            action = Some(SummaryAction::Close);
        }
    });
    action
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: IMP-056, IMP-057
    #[test]
    fn the_summary_of_a_note_is_the_sender_the_place_or_the_first_text() {
        let mail = "---\ntype: email\nsubject: \"x\"\nfrom: \"Hans Müller <h@x.de>\"\ndate: 2026-11-13 10:22\n---\n\n# x\n\n## Message\n\nHallo\n";
        assert_eq!(line_of(mail), "Hans Müller · 2026-11-13 10:22");
        assert_eq!(line_of("---\ntype: event\nstart: 2026-12-02 10:00\nlocation: \"Raum 1.12\"\n---\n\n# Plan\n"), "2026-12-02 10:00 · Raum 1.12");
        assert_eq!(line_of("---\ntype: event\nstart: 2026-12-02 10:00\n---\n"), "2026-12-02 10:00", "a missing part is left out");
        assert_eq!(line_of("# Title\n\n- [[a]]\n![[pic.png]]\n---\n> a quote  with   spaces\nlater"), "a quote with spaces");
        assert_eq!(line_of("---\ntags: [a]\n---\n\n# Only a title\n"), "");
        let long = format!("{} end", "word ".repeat(40));
        let line = line_of(&long);
        assert!(line.ends_with('…') && line.chars().count() <= LINE_MAX + 1, "{line}");
        assert_eq!(line_of(""), "");
    }

    // spec: IMP-056, MEDIA-019
    #[test]
    fn the_title_counts_notes_and_files() {
        let s = |n: usize, f: usize, l: usize| ImportSummary {
            notes: (0..n).map(|_| Entry { rel: PathBuf::new(), name: String::new(), line: String::new() }).collect(),
            files: vec![PathBuf::new(); f],
            linked: vec![crate::import::Linked { link: String::new(), path: PathBuf::new() }; l],
            errors: vec![],
            place: String::new(),
        };
        assert_eq!(s(1, 0, 0).title(), "Imported 1 note");
        assert_eq!(s(3, 1, 0).title(), "Imported 3 notes and 1 file");
        assert_eq!(s(0, 2, 0).title(), "Imported 2 files");
        // MEDIA-019: files that stay where they are are counted apart
        assert_eq!(s(0, 0, 1).title(), "Imported 1 linked file");
        assert_eq!(s(0, 0, 3).title(), "Imported 3 linked files");
        assert_eq!(s(3, 2, 1).title(), "Imported 3 notes, 2 files and 1 linked file");
    }
}
