//! The picker window (NAV-020, NAV-021): a text field and a list of hits that the typing
//! filters, driven by the keyboard. What it lists comes from a [`Source`]: the names of the
//! files of the vault (`Space f f`) or the text of everything indexed (`Space f g`, NAV-022);
//! the window, the keys and the way a pick is carried out are the same for both.

use crate::app::Action;
use crate::dialog::Dialog;
use crate::markdown;
use crate::search_all::{self, Ctx};
use crate::theme;
use crate::vault::rel_to_string;
use eframe::egui::{self, Context, FontId, Key, Modifiers, RichText, Ui};
use std::time::{Duration, Instant};

/// How many hits are listed at most.
const MAX_HITS: usize = 14;

/// One line of the list. `action` is what picking it does (open a file, jump to a line, show a
/// message, …): the app carries it out like any other action.
pub struct Hit {
    /// the group the line is listed under ("Notes", "Mail", "Posts"); empty: no groups
    pub group: &'static str,
    pub title: String,
    pub detail: String,
    pub action: Action,
}

/// Where the hits come from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// File names only, fuzzy (NAV-020); never creates a note.
    FileNames,
    /// The text of notes, mail and posts, with wildcards, grouped (NAV-022, NAV-023).
    Everything,
}

impl Source {
    /// The window title.
    pub fn title(self) -> &'static str {
        match self {
            Source::FileNames => "Find a file",
            Source::Everything => "Search notes, mail and posts",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Source::FileNames => "File name…",
            Source::Everything => "Text to find (* and ? are wildcards)…",
        }
    }

    /// Does a keystroke wait before the list is made again (NAV-024)? Names are found at once.
    fn debounced(self) -> bool {
        matches!(self, Source::Everything)
    }

    /// The hits for what is typed, best first.
    pub fn hits(self, ctx: &Ctx, query: &str) -> Vec<Hit> {
        match self {
            Source::FileNames => {
                let mut found: Vec<(i32, Hit)> = ctx
                    .vault
                    .files
                    .iter()
                    .filter_map(|f| {
                        let name = f.display_name();
                        let score = markdown::fuzzy_score(query.trim(), &name)?;
                        let detail = f.rel.parent().map(rel_to_string).unwrap_or_default();
                        Some((score, Hit { group: "", title: name, detail, action: Action::Open { rel: f.rel.clone(), new_tab: false } }))
                    })
                    .collect();
                found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
                found.into_iter().take(MAX_HITS).map(|(_, h)| h).collect()
            }
            Source::Everything => search_all::all_hits(ctx, query),
        }
    }
}

/// The state of an open picker.
pub struct Picker {
    pub source: Source,
    pub query: String,
    pub sel: usize,
    /// the list made for `made_for`; made again when the typing has settled
    hits: Vec<Hit>,
    made_for: Option<String>,
    made_at: Option<Instant>,
}

/// A list is made at most this often while typing (NAV-024).
const DEBOUNCE: Duration = Duration::from_millis(150);

impl Picker {
    pub fn new(source: Source) -> Picker {
        Picker { source, query: String::new(), sel: 0, hits: Vec::new(), made_for: None, made_at: None }
    }

    /// Make the list for the query when it is not the one the list is for and either the source
    /// is not debounced, or the last list was made long enough ago, or `now` is asked for.
    /// True while the list is for an older query.
    fn refresh(&mut self, ctx: &Ctx, now: bool) -> bool {
        if self.made_for.as_deref() == Some(self.query.as_str()) {
            return false;
        }
        let due = now || !self.source.debounced() || self.made_at.is_none_or(|t| t.elapsed() >= DEBOUNCE);
        if due {
            self.hits = self.source.hits(ctx, &self.query);
            self.made_for = Some(self.query.clone());
            self.made_at = Some(Instant::now());
            self.sel = 0;
        }
        !due
    }

    /// The hits now listed (tests).
    #[cfg(test)]
    pub fn listed(&self) -> &[Hit] {
        &self.hits
    }
}

/// The heading of each group of the list with its count: "Notes  3", "Mail  20+" when the group
/// is cut at its limit (NAV-022). A list without groups has none.
fn group_headers(hits: &[Hit]) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, usize)> = Vec::new();
    for h in hits.iter().filter(|h| !h.group.is_empty()) {
        match out.iter_mut().find(|(g, _)| *g == h.group) {
            Some((_, n)) => *n += 1,
            None => out.push((h.group, 1)),
        }
    }
    out.into_iter().map(|(g, n)| (g, format!("{g}  {n}{}", if n >= search_all::GROUP_LIMIT { "+" } else { "" }))).collect()
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
pub fn show(ctx: &Context, p: &mut Picker, data: &Ctx) -> Outcome {
    let mut out = Outcome { pick: None, close: false };
    let resp = Dialog::new("picker", p.source.title()).width(520.0).show(ctx, |ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut p.query).hint_text(p.source.hint()).desired_width(f32::INFINITY).font(FontId::proportional(16.0)));
        r.request_focus();
        let keys = ListKeys::take(ui);
        // Enter means the list of what is typed, not of what was typed before
        if p.refresh(data, keys.enter) {
            ui.ctx().request_repaint_after(DEBOUNCE);
        }
        keys.select(&mut p.sel, p.hits.len());
        ui.add_space(6.0);
        let mut pick = (keys.enter && !p.hits.is_empty()).then_some(p.sel);
        let headers = group_headers(&p.hits);
        let mut last = "";
        for (i, h) in p.hits.iter().enumerate() {
            if h.group != last {
                last = h.group;
                if let Some((_, text)) = headers.iter().find(|(g, _)| *g == last) {
                    ui.label(RichText::new(text).color(theme::palette().muted).size(12.0));
                }
            }
            let label = if h.detail.is_empty() { h.title.clone() } else { format!("{}    {}", h.title, h.detail) };
            let resp = ui.selectable_label(i == p.sel, label);
            if i == p.sel && (keys.down || keys.up) {
                resp.scroll_to_me(None);
            }
            if resp.clicked() {
                pick = Some(i);
            }
        }
        if p.hits.is_empty() && p.made_for.is_some() {
            ui.label(RichText::new("Nothing found").color(theme::palette().muted).size(12.5));
        }
        ui.label(RichText::new("↑↓ or Ctrl+J / Ctrl+K to move · Enter to open · Esc to close").color(theme::palette().muted).size(11.5));
        out.pick = pick.map(|i| p.hits.swap_remove(i));
        out.close = keys.esc;
    });
    out.close |= resp.should_close();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::social::post_index::PostIndex;
    use crate::vault::Vault;

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
        let posts = PostIndex::in_memory();
        let ctx = Ctx { vault: &v, mail: None, posts: &posts };
        let titles = |q: &str| Source::FileNames.hits(&ctx, q).into_iter().map(|h| h.title).collect::<Vec<_>>();
        assert_eq!(titles("road"), ["Roadmap", "Road trip"], "names starting with it, the shorter first");
        assert_eq!(titles("rdmp"), ["Roadmap"], "fuzzy: letters in order");
        assert_eq!(titles("GROC"), ["Groceries"], "case does not matter");
        assert!(titles("banana").is_empty(), "the text of a note is not searched");
        assert!(titles("projects").is_empty(), "nor the folder");
        assert_eq!(titles("").len(), 3, "no query lists the files");
        let hit = Source::FileNames.hits(&ctx, "roadm").remove(0);
        assert_eq!(hit.detail, "projects");
        assert!(matches!(hit.action, Action::Open { ref rel, new_tab: false } if rel == std::path::Path::new("projects/Roadmap.md")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: NAV-024
    #[test]
    fn the_list_of_the_everything_source_waits_for_the_typing_to_settle() {
        let dir = std::env::temp_dir().join(format!("shale-picker-debounce-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("A.md"), "banana\nberry\n").unwrap();
        let v = Vault::open(&dir).unwrap();
        let posts = PostIndex::in_memory();
        let ctx = Ctx { vault: &v, mail: None, posts: &posts };
        let mut p = Picker::new(Source::Everything);
        assert!(!p.refresh(&ctx, false), "nothing typed, nothing waits");
        std::thread::sleep(DEBOUNCE + Duration::from_millis(20));
        p.query = "b".into();
        assert!(!p.refresh(&ctx, false), "a key after a pause is answered at once");
        assert_eq!(p.listed().len(), 2);
        p.query = "ba".into();
        assert!(p.refresh(&ctx, false), "the next key right after it waits");
        assert_eq!(p.listed().len(), 2, "the old list stays meanwhile");
        assert!(!p.refresh(&ctx, true), "Enter makes the list of what is typed");
        assert_eq!(p.listed().len(), 1);
        p.query = "ban".into();
        assert!(p.refresh(&ctx, false));
        std::thread::sleep(DEBOUNCE + Duration::from_millis(20));
        assert!(!p.refresh(&ctx, false), "later the list is made");
        assert_eq!(p.listed()[0].title, "banana");
        // file names are never held back
        let mut f = Picker::new(Source::FileNames);
        f.query = "a".into();
        f.refresh(&ctx, false);
        f.query = "".into();
        assert!(!f.refresh(&ctx, false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: NAV-022
    #[test]
    fn groups_have_a_heading_with_their_count() {
        let hit = |group| Hit { group, title: String::new(), detail: String::new(), action: Action::FocusEditor };
        let hits = [hit("Notes"), hit("Notes"), hit("Posts")];
        assert_eq!(group_headers(&hits), [("Notes", "Notes  2".to_string()), ("Posts", "Posts  1".to_string())]);
        let full: Vec<Hit> = (0..search_all::GROUP_LIMIT).map(|_| hit("Mail")).collect();
        assert_eq!(group_headers(&full)[0].1, "Mail  20+", "a group at its limit may have more");
        assert!(group_headers(&[hit("")]).is_empty(), "no groups, no headings");
    }
}
