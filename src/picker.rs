//! The picker window (NAV-020, NAV-021): a text field and a list of hits that the typing
//! filters, driven by the keyboard. What it lists comes from a [`Source`]: the names of the
//! files of the vault (`Space f f`) or the text of everything indexed (`Space f g`, NAV-022);
//! the window, the keys and the way a pick is carried out are the same for both.

use crate::app::Action;
use crate::dialog::Dialog;
use crate::keys::{Action as KeyAction, Keymap};
use crate::markdown;
use crate::search_all::{self, Ctx, Query, Refreshed, Results};
use crate::theme;
use crate::vault::rel_to_string;
use eframe::egui::{self, Context, FontId, RichText, Ui};

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
            Source::Everything => search_all::all_hits(ctx, &Query::picker(query)),
        }
    }
}

/// The state of an open picker.
pub struct Picker {
    pub source: Source,
    pub query: String,
    pub sel: usize,
    /// the list made for the query; made again when the typing has settled (NAV-024)
    results: Results,
}

impl Picker {
    pub fn new(source: Source) -> Picker {
        Picker { source, query: String::new(), sel: 0, results: Results::default() }
    }

    /// Make the list for the query when it is not the one the list is for and either the source
    /// is not debounced, or the last list was made long enough ago, or `now` is asked for.
    /// True while the list is for an older query.
    fn refresh(&mut self, ctx: &Ctx, now: bool) -> bool {
        let (source, query) = (self.source, &self.query);
        let done = self.results.refresh(query, ctx.stamp(), source.debounced(), now, || source.hits(ctx, query));
        if done == Refreshed::Made {
            self.sel = 0;
        }
        done == Refreshed::Waits
    }

    /// The hits now listed (tests).
    #[cfg(test)]
    pub fn listed(&self) -> &[Hit] {
        self.results.hits()
    }
}

/// The keys every list window of the app reads (NAV-049): the actions of the scope
/// `Scope::ListWindow` of the keymap, by default ↓ / Ctrl+J, ↑ / Ctrl+K, Enter and Esc.
pub struct ListKeys {
    pub down: bool,
    pub up: bool,
    pub enter: bool,
    pub esc: bool,
}

impl ListKeys {
    pub fn take(ui: &mut Ui, keymap: &Keymap) -> ListKeys {
        ui.input_mut(|i| ListKeys { down: keymap.take(i, KeyAction::ListNext), up: keymap.take(i, KeyAction::ListPrev), enter: keymap.take(i, KeyAction::ListOpen), esc: keymap.take(i, KeyAction::ListClose) })
    }

    /// The hint of the window, from the keys as they are bound now (NAV-049, like MAIL-072): "↓ / Ctrl+J
    /// next · ↑ / Ctrl+K previous · Enter open · Esc close"; an action without a key is left out.
    pub fn hint(keymap: &Keymap) -> String {
        [(KeyAction::ListNext, "next"), (KeyAction::ListPrev, "previous"), (KeyAction::ListOpen, "open"), (KeyAction::ListClose, "close")]
            .into_iter()
            .filter_map(|(a, what)| keymap.hint(a).map(|k| format!("{k} {what}")))
            .collect::<Vec<_>>()
            .join(" · ")
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

/// What the header of the window passes to its list: the keys it took this frame.
struct Frame<'a> {
    p: &'a mut Picker,
    keys: ListKeys,
}

/// Draw the window: the field and the key hint are its header (NAV-049, they stay in view), the
/// list scrolls below them.
pub fn show(ctx: &Context, p: &mut Picker, data: &Ctx, keymap: &Keymap) -> Outcome {
    let mut out = Outcome { pick: None, close: false };
    let mut frame = Frame { p, keys: ListKeys { down: false, up: false, enter: false, esc: false } };
    let title = frame.p.source.title();
    let resp = Dialog::new("picker", title).width(520.0).show_with_header(
        ctx,
        &mut frame,
        |ui, f| {
            let r = ui.add(egui::TextEdit::singleline(&mut f.p.query).hint_text(f.p.source.hint()).desired_width(f32::INFINITY).font(FontId::proportional(16.0)));
            r.request_focus();
            f.keys = ListKeys::take(ui, keymap);
            theme::prose(ui, RichText::new(ListKeys::hint(keymap)).color(theme::palette().muted).size(11.5));
        },
        |ui, f| {
            let (p, keys) = (&mut *f.p, &f.keys);
            // Enter means the list of what is typed, not of what was typed before
            if p.refresh(data, keys.enter) {
                ui.ctx().request_repaint_after(search_all::DEBOUNCE);
            }
            keys.select(&mut p.sel, p.results.hits().len());
            let mut pick = (keys.enter && !p.results.hits().is_empty()).then_some(p.sel);
            let headers = search_all::group_headers(p.results.hits(), search_all::GROUP_LIMIT);
            let mut last = "";
            for (i, h) in p.results.hits().iter().enumerate() {
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
            if p.results.hits().is_empty() && p.results.is_made() {
                ui.label(RichText::new("Nothing found").color(theme::palette().muted).size(12.5));
            }
            out.pick = pick.map(|i| p.results.take(i));
            out.close = keys.esc;
        },
    );
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
        std::thread::sleep(search_all::DEBOUNCE + std::time::Duration::from_millis(20));
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
        std::thread::sleep(search_all::DEBOUNCE + std::time::Duration::from_millis(20));
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
}
