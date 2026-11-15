//! The picker window (NAV-020, NAV-021): a text field and a list of hits that the typing
//! filters, driven by the keyboard. What it lists comes from a [`Source`]; today there is one,
//! the names of the files of the vault (`Space f f`). A later source (full text over notes, mail
//! and posts) is one more variant of [`Source`] with its own `hits`; the window, the keys and the
//! way a pick is carried out stay as they are.

use crate::app::Action;
use crate::dialog::Dialog;
use crate::markdown;
use crate::theme;
use crate::vault::{rel_to_string, Vault};
use eframe::egui::{self, Context, FontId, Key, Modifiers, RichText, Ui};

/// How many hits are listed at most.
const MAX_HITS: usize = 14;

/// One line of the list. `action` is what picking it does (open a file, jump to a line, show a
/// message, …): the app carries it out like any other action.
pub struct Hit {
    pub title: String,
    pub detail: String,
    pub action: Action,
}

/// Where the hits come from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// File names only, fuzzy (NAV-020); never creates a note.
    FileNames,
}

impl Source {
    /// The window title.
    pub fn title(self) -> &'static str {
        match self {
            Source::FileNames => "Find a file",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Source::FileNames => "File name…",
        }
    }

    /// The hits for what is typed, best first.
    pub fn hits(self, vault: &Vault, query: &str) -> Vec<Hit> {
        match self {
            Source::FileNames => {
                let mut found: Vec<(i32, Hit)> = vault
                    .files
                    .iter()
                    .filter_map(|f| {
                        let name = f.display_name();
                        let score = markdown::fuzzy_score(query.trim(), &name)?;
                        let detail = f.rel.parent().map(rel_to_string).unwrap_or_default();
                        Some((score, Hit { title: name, detail, action: Action::Open { rel: f.rel.clone(), new_tab: false } }))
                    })
                    .collect();
                found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
                found.into_iter().take(MAX_HITS).map(|(_, h)| h).collect()
            }
        }
    }
}

/// The state of an open picker.
pub struct Picker {
    pub source: Source,
    pub query: String,
    pub sel: usize,
}

impl Picker {
    pub fn new(source: Source) -> Picker {
        Picker { source, query: String::new(), sel: 0 }
    }
}

/// The keys every list window of the app reads: ↓ / ↑ (also Ctrl+J / Ctrl+K), Enter, Esc.
pub struct ListKeys {
    pub down: bool,
    pub up: bool,
    pub enter: bool,
    pub esc: bool,
}

impl ListKeys {
    pub fn take(ui: &mut Ui) -> ListKeys {
        ui.input_mut(|i| ListKeys {
            down: i.consume_key(Modifiers::NONE, Key::ArrowDown) | i.consume_key(Modifiers::COMMAND, Key::J),
            up: i.consume_key(Modifiers::NONE, Key::ArrowUp) | i.consume_key(Modifiers::COMMAND, Key::K),
            enter: i.consume_key(Modifiers::NONE, Key::Enter),
            esc: i.consume_key(Modifiers::NONE, Key::Escape),
        })
    }

    /// Move the selection among `n` lines, wrapping around.
    pub fn select(&self, sel: &mut usize, n: usize) {
        if n > 0 {
            if self.down {
                *sel = (*sel + 1) % n;
            }
            if self.up {
                *sel = (*sel + n - 1) % n;
            }
            *sel = (*sel).min(n - 1);
        }
    }
}

/// What the user did in this frame.
pub struct Outcome {
    /// The hit that was picked (Enter or a click): the window closes.
    pub pick: Option<Hit>,
    pub close: bool,
}

/// Draw the window.
pub fn show(ctx: &Context, p: &mut Picker, vault: &Vault) -> Outcome {
    let mut out = Outcome { pick: None, close: false };
    let resp = Dialog::new("picker", p.source.title()).width(520.0).show(ctx, |ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut p.query).hint_text(p.source.hint()).desired_width(f32::INFINITY).font(FontId::proportional(16.0)));
        r.request_focus();
        let mut hits = p.source.hits(vault, &p.query);
        let keys = ListKeys::take(ui);
        keys.select(&mut p.sel, hits.len());
        ui.add_space(6.0);
        let mut pick = (keys.enter && !hits.is_empty()).then_some(p.sel);
        for (i, h) in hits.iter().enumerate() {
            let label = if h.detail.is_empty() { h.title.clone() } else { format!("{}    {}", h.title, h.detail) };
            let resp = ui.selectable_label(i == p.sel, label);
            if i == p.sel && (keys.down || keys.up) {
                resp.scroll_to_me(None);
            }
            if resp.clicked() {
                pick = Some(i);
            }
        }
        if hits.is_empty() {
            ui.label(RichText::new("Nothing found").color(theme::MUTED).size(12.5));
        }
        ui.label(RichText::new("↑↓ or Ctrl+J / Ctrl+K to move · Enter to open · Esc to close").color(theme::MUTED).size(11.5));
        out.pick = pick.map(|i| hits.swap_remove(i));
        out.close = keys.esc;
    });
    out.close |= resp.should_close();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: NAV-020
    #[test]
    fn file_names_are_matched_fuzzy_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("shale-picker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("projects")).unwrap();
        std::fs::write(dir.join("projects/Roadmap.md"), "the banana plan\n").unwrap();
        std::fs::write(dir.join("Groceries.md"), "# Roadmap\n").unwrap();
        std::fs::write(dir.join("Road trip.md"), "x\n").unwrap();
        let v = Vault::open(&dir).unwrap();
        let titles = |q: &str| Source::FileNames.hits(&v, q).into_iter().map(|h| h.title).collect::<Vec<_>>();
        assert_eq!(titles("road"), ["Road trip", "Roadmap"], "a name that starts with it first");
        assert_eq!(titles("rdmp"), ["Roadmap"], "fuzzy: letters in order");
        assert_eq!(titles("GROC"), ["Groceries"], "case does not matter");
        assert!(titles("banana").is_empty(), "the text of a note is not searched");
        assert!(titles("projects").is_empty(), "nor the folder");
        assert_eq!(titles("").len(), 3, "no query lists the files");
        let hit = Source::FileNames.hits(&v, "roadm").remove(0);
        assert_eq!(hit.detail, "projects");
        assert!(matches!(hit.action, Action::Open { ref rel, new_tab: false } if rel == std::path::Path::new("projects/Roadmap.md")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
