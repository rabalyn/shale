//! The picker window (NAV-020, NAV-021): a text field and a list of hits that the typing
//! filters, driven by the keyboard. What it lists comes from a [`Source`]: the names of the
//! files of the vault (`Space f f`) or the text of everything indexed (`Space f g`, NAV-022);
//! the window, the keys and the way a pick is carried out are the same for both.

use crate::app::Action;
use crate::dialog::Dialog;
use crate::keys::{Action as KeyAction, Keymap};
use crate::markdown;
use crate::search_all::{self, Ask, Ctx, Query, Refreshed, Request, Results, SearchWorker, Slot, Start};
use crate::search_render::Look;
use crate::search_sources::{self, Bar, Filter, SearchConfig, Selection};
use crate::theme;
use crate::vault::rel_to_string;
use eframe::egui::{self, Color32, Context, FontId, RichText, Ui};
use std::sync::Arc;

/// The name of the command of the quick switcher that opens the vault list (VAULT-044).
const SWITCH_VAULT: &str = "Switch vault";

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
    /// the found text: byte ranges of `title`, drawn marked (NAV-057)
    pub marks: Vec<std::ops::Range<usize>>,
    /// the title as a job, made when the hit is first drawn (NAV-056)
    pub(crate) jobs: crate::search_render::Jobs,
}

impl Hit {
    pub fn new(group: &'static str, title: String, detail: String, action: Action) -> Hit {
        Hit { group, title, detail, action, marks: Vec::new(), jobs: Default::default() }
    }

    /// The same hit with the found text marked.
    pub fn marked(self, marks: Vec<std::ops::Range<usize>>) -> Hit {
        Hit { marks, ..self }
    }
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

    /// The hits for what is typed, best first: the file names fuzzy-matched. The search of
    /// everything is not asked here but of the worker ([`Source::start`]).
    fn file_names(ctx: &Ctx, query: &str) -> Vec<Hit> {
        let mut found: Vec<(i32, Hit)> = ctx
            .vault
            .files
            .iter()
            .filter_map(|f| {
                let name = f.display_name();
                let score = markdown::fuzzy_score(query.trim(), &name)?;
                let detail = f.rel.parent().map(rel_to_string).unwrap_or_default();
                Some((score, Hit::new("", name, detail, Action::Open { rel: f.rel.clone(), new_tab: false })))
            })
            .collect();
        // the command that shows the list of the vaults (VAULT-044): found by a part of its name
        let typed = query.trim();
        if let Some(score) = markdown::fuzzy_score(typed, SWITCH_VAULT).filter(|s| !typed.is_empty() && *s >= 500) {
            found.push((score, Hit::new("", SWITCH_VAULT.to_string(), "Command".to_string(), Action::OpenVaultMenu)));
        }
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
        found.into_iter().take(MAX_HITS).map(|(_, h)| h).collect()
    }

    /// The hits for what is typed, here and now (tests): the search of everything lists every
    /// source.
    #[cfg(test)]
    pub fn hits(self, ctx: &Ctx, query: &str) -> Vec<Hit> {
        match self {
            Source::FileNames => Source::file_names(ctx, query),
            Source::Everything => search_all::all_hits(ctx, &Query::picker(query)),
        }
    }

    /// Make the list for `query`: the names of files are found now, the search of everything is
    /// given to the worker (QUERY-014), which counts every source.
    fn start(self, ctx: &Ctx, query: &Query, worker: &mut SearchWorker) -> Start {
        match self {
            Source::FileNames => Start::Done(search_all::Found { hits: Source::file_names(ctx, query.text), ..Default::default() }),
            Source::Everything => Start::Queued(worker.submit(Slot::PICKER, Request::new(ctx, query))),
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
    /// the check of the text, made again only when the text changes (QUERY-013)
    check: Box<search_all::query::Cache>,
    /// the solo of the count buttons (NAV-052); the default selection is in the settings
    pub filter: Filter,
    /// the filter form behind the ⚙ button (QUERY-027) and whether it is open
    pub form: Box<crate::search_form::State>,
    pub form_open: bool,
}

impl Picker {
    pub fn new(source: Source) -> Picker {
        Picker { source, query: String::new(), sel: 0, results: Results::default(), check: Default::default(), filter: Filter::default(), form: Default::default(), form_open: false }
    }

    /// Make the list for the query when it is not the one the list is for and either the source
    /// is not debounced, or the last list was made long enough ago, or `now` is asked for.
    /// True while the list is for an older query.
    fn refresh(&mut self, ctx: &Ctx, worker: &mut SearchWorker, now: bool, cfg: &SearchConfig) -> bool {
        let source = self.source;
        // the text, with the letters of the prefixes and the sources to list, and its check
        let prefixes = cfg.prefixes();
        let query = Query::picker(&self.query).with_sources(prefixes, self.filter.shown(cfg.selection()));
        let query = if source == Source::Everything { query.with_checked(self.check.get(&self.query, prefixes)) } else { query };
        let listed = if source == Source::Everything { query.listed() } else { Selection::ALL };
        let blank = source == Source::Everything && query.is_blank();
        let invalid = query.checked.is_some_and(|c| c.error().is_some());
        let ask = Ask { slot: Slot::PICKER, text: query.text, stamp: ctx.stamp(), debounced: source.debounced(), now, listed, blank, invalid };
        let done = self.results.refresh(worker, &ask, |w| source.start(ctx, &query, w));
        if matches!(done, Refreshed::Made | Refreshed::Relisted) {
            self.sel = 0;
        }
        done == Refreshed::Waits
    }

    /// The hits now listed (tests).
    #[cfg(test)]
    pub fn listed(&self) -> &[Hit] {
        self.results.hits()
    }

    /// The text the list was made for (tests wait for it: a debounced query is no job of the worker, DEV-041).
    #[cfg(test)]
    pub(crate) fn listed_for(&self) -> Option<&str> {
        self.results.made_for_text()
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

    /// Only the key that closes the window: the keys of the list belong to the form while it has the keyboard.
    pub fn take_close(ui: &mut Ui, keymap: &Keymap) -> ListKeys {
        ui.input_mut(|i| ListKeys { down: false, up: false, enter: false, esc: keymap.take(i, KeyAction::ListClose) })
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

/// The keys of the search of everything that choose the sources (NAV-055): the actions
/// `list.next_source` and `list.prev_source` of the scope `Scope::ListWindow`, by default Tab and
/// Shift+Tab.
pub struct SourceKeys {
    pub next: bool,
    pub prev: bool,
}

impl SourceKeys {
    pub fn take(ui: &mut Ui, keymap: &Keymap) -> SourceKeys {
        // (Shift+Tab first: a key of the same letter without Shift would take it too)
        ui.input_mut(|i| {
            let prev = keymap.take(i, KeyAction::ListSourcePrev);
            SourceKeys { prev, next: !prev && keymap.take(i, KeyAction::ListSourceNext) }
        })
    }

    /// The hint of the two keys as they are bound now: "Tab next source · Shift+Tab previous source".
    pub fn hint(keymap: &Keymap) -> String {
        [(KeyAction::ListSourceNext, "next source"), (KeyAction::ListSourcePrev, "previous source")].into_iter().filter_map(|(a, what)| keymap.hint(a).map(|k| format!("{k} {what}"))).collect::<Vec<_>>().join(" · ")
    }
}

/// What the user did in this frame.
pub struct Outcome {
    /// The hit that was picked (Enter or a click): the window closes.
    pub pick: Option<Hit>,
    pub close: bool,
    /// The default selection of the sources was changed (Shift+click): save the settings.
    pub save: bool,
}

/// What the header of the window passes to its list: the keys it took this frame.
struct Frame<'a> {
    p: &'a mut Picker,
    worker: &'a mut SearchWorker,
    keys: ListKeys,
    save: bool,
}

/// Draw the window: the field, the key hint and the source buttons are its header (NAV-049, they
/// stay in view), the list scrolls below them. The list is made in the header, so that the
/// counts of its buttons are those of the list below. `rendered`: the lines of notes are drawn
/// rendered, not raw (NAV-056, NAV-058).
pub fn show(ctx: &Context, p: &mut Picker, data: &Ctx, worker: &mut SearchWorker, keymap: &Keymap, cfg: &mut SearchConfig, rendered: bool) -> Outcome {
    let mut out = Outcome { pick: None, close: false, save: false };
    let mut frame = Frame { p, worker, keys: ListKeys { down: false, up: false, enter: false, esc: false }, save: false };
    let title = frame.p.source.title();
    let resp = Dialog::new("picker", title).width(520.0).show_with_header(
        ctx,
        &mut frame,
        |ui, f| {
            let everything = f.p.source == Source::Everything;
            // (while a widget of the form has the keyboard, the keys of the list are the form's, except the one that closes the window)
            let box_id = egui::Id::unique("picker-box");
            let in_form = everything && f.p.form_open && ui.memory(|m| m.focused()).is_some_and(|id| id != box_id);
            // (Tab is no focus change here: the field keeps it for the keys of the sources)
            let edit = egui::TextEdit::singleline(&mut f.p.query).id(box_id).hint_text(f.p.source.hint()).font(FontId::proportional(16.0)).lock_focus(true);
            // (the search of everything underlines what is wrong with the query, QUERY-008, and has the ⚙ button of the form, QUERY-027)
            let (r, gear) = {
                let mut layouter = search_sources::error_layouter(&mut f.p.check, cfg.prefixes(), Some(16.0));
                if everything {
                    ui.horizontal(|ui| {
                        let width = ui.available_width() - 34.0;
                        let r = ui.add(edit.desired_width(width).layouter(&mut layouter));
                        let gear = ui.add(egui::Button::new("⚙").selected(f.p.form_open)).on_hover_text("Filter form");
                        (r, Some(gear))
                    })
                    .inner
                } else {
                    (ui.add(edit.desired_width(f32::INFINITY)), None)
                }
            };
            if !f.p.form_open || ui.memory(|m| m.focused().is_none()) {
                r.request_focus();
            }
            // (Esc closes an open list or menu of the form first, not the window)
            // (egui takes the keyboard from a field on Esc, so a list is told by itself)
            let form_open = crate::search_form::list_open(ui.ctx()) || (in_form && egui::Popup::is_any_open(ui.ctx()));
            f.keys = if form_open {
                ListKeys { down: false, up: false, enter: false, esc: false }
            } else if in_form {
                ListKeys::take_close(ui, keymap)
            } else {
                ListKeys::take(ui, keymap)
            };
            let form_key = everything && ui.input_mut(|i| keymap.take(i, KeyAction::ListForm));
            if gear.is_some_and(|g| g.clicked()) || form_key {
                f.p.form_open = !f.p.form_open;
                if f.p.form_open {
                    f.p.form.focus();
                }
            }
            let source_keys = if everything && !in_form { SourceKeys::take(ui, keymap) } else { SourceKeys { next: false, prev: false } };
            let prefixed = cfg.prefixes().parse(&f.p.query).prefix.is_some();
            let available = Bar::available(data.mail.is_some());
            if !prefixed && (source_keys.next || source_keys.prev) {
                f.p.filter.step(source_keys.next, &available);
            }
            // Enter means the list of what is typed, not of what was typed before
            if f.p.refresh(data, f.worker, f.keys.enter, cfg) {
                ui.ctx().request_repaint_after(search_all::DEBOUNCE);
            }
            let mut hint = ListKeys::hint(keymap);
            if everything {
                let more = SourceKeys::hint(keymap);
                if !more.is_empty() {
                    hint = format!("{hint} · {more}");
                }
            }
            theme::prose(ui, RichText::new(hint).color(theme::palette().muted).size(11.5));
            if everything {
                let listed = Query::picker(&f.p.query).with_sources(cfg.prefixes(), f.p.filter.shown(cfg.selection())).listed();
                let bar = Bar { counts: f.p.results.counts(), limit: search_all::GROUP_LIMIT, mail_open: data.mail.is_some(), listed, prefixed, searching: f.p.results.is_pending() };
                let before = f.p.filter;
                if search_sources::bar(ui, &bar, &mut f.p.filter, cfg) {
                    f.save = true;
                }
                let skipped: Vec<String> = f.p.results.skipped(listed).map(search_all::Skipped::message).collect();
                search_sources::notices(ui, f.p.check.get(&f.p.query, cfg.prefixes()).error(), &skipped, f.p.results.stopped_early());
                if f.save || f.p.filter != before {
                    // the list of the new sources, in this frame
                    f.p.refresh(data, f.worker, false, cfg);
                }
                if f.p.form_open {
                    let count = f.p.results.counts().map(|c| c.matches_text(listed, search_all::GROUP_LIMIT));
                    let mut opts = crate::search_form::Opts { vault: data.vault, worker: &mut *f.worker, prefixes: cfg.prefixes(), host_box: true, count: count.as_deref() };
                    let changed = crate::search_form::show(ui, &mut f.p.form, &mut f.p.query, &mut f.p.check, &mut cfg.advanced, None, &mut opts);
                    f.save |= changed.advanced;
                    if changed.query {
                        // what the form wrote is searched like what was typed, at once
                        f.p.refresh(data, f.worker, true, cfg);
                    }
                }
            }
        },
        |ui, f| {
            let (p, keys) = (&mut *f.p, &f.keys);
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
                // the title as a job (made once per hit, NAV-056), then the detail
                let size = ui.style().text_styles[&egui::TextStyle::Body].size;
                let mut job = Arc::unwrap_or_clone(h.job(Look { rendered, size, raw_colour: Color32::PLACEHOLDER }));
                if !h.detail.is_empty() {
                    job.append(&format!("    {}", h.detail), 0.0, egui::TextFormat { font_id: FontId::proportional(size), color: theme::palette().muted, ..Default::default() });
                }
                let resp = ui.selectable_label(i == p.sel, job.clone());
                // (the text sits at the left padding, centred in the row when it is one, at the top when it wraps)
                let pad = ui.spacing().button_padding;
                let wrapped = resp.rect.height() > ui.spacing().interact_size.y + 1.0;
                crate::search_render::paint_task_boxes(ui, &job, |g| egui::pos2(resp.rect.min.x + pad.x, if wrapped { resp.rect.min.y + pad.y } else { resp.rect.center().y - g.size().y / 2.0 }));
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
    out.save = frame.save;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::social::post_index::PostIndex;
    use crate::vault::Vault;

    // spec: NAV-020, NAV-021
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

    // spec: NAV-020, NAV-021
    #[test]
    fn a_source_gives_hits_of_a_group_a_title_a_detail_and_an_action_and_the_file_names_stop_at_14() {
        let dir = std::env::temp_dir().join(format!("shale-picker-sources-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..20 {
            std::fs::write(dir.join(format!("Plan {i:02}.md")), "the plan\n").unwrap();
        }
        let v = Vault::open(&dir).unwrap();
        let posts = PostIndex::in_memory();
        let ctx = Ctx { vault: &v, mail: None, posts: &posts };
        assert_eq!(Source::FileNames.hits(&ctx, "plan").len(), 14, "at most 14 names");
        // two sources, one kind of hit: the window and the keys do not depend on the source
        let names = Source::FileNames.hits(&ctx, "plan 01").remove(0);
        let text = Source::Everything.hits(&ctx, "the plan").remove(0);
        assert_eq!((names.group, names.title.as_str()), ("", "Plan 01"));
        assert_eq!(text.group, "Notes");
        for h in [&names, &text] {
            assert!(matches!(h.action, Action::Open { .. } | Action::GotoMatch { .. }), "a pick opens the note: {:?}", h.title);
        }
        assert_ne!((Source::FileNames.title(), Source::FileNames.hint()), (Source::Everything.title(), Source::Everything.hint()), "a source has its own title and hint");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Refresh until the worker has answered (the picker is asked every frame in the window).
    fn wait_for_list(p: &mut Picker, ctx: &Ctx, w: &mut SearchWorker, cfg: &SearchConfig) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while p.results.is_pending() {
            p.refresh(ctx, w, false, cfg);
            assert!(std::time::Instant::now() < end, "the worker never answered");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    // spec: NAV-024, QUERY-014, QUERY-018
    #[test]
    fn the_list_of_the_everything_source_waits_for_the_typing_to_settle() {
        let dir = std::env::temp_dir().join(format!("shale-picker-debounce-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("A.md"), "banana\nberry\n").unwrap();
        let v = Vault::open(&dir).unwrap();
        let posts = PostIndex::in_memory();
        let ctx = Ctx { vault: &v, mail: None, posts: &posts };
        let cfg = SearchConfig::default();
        let mut w = SearchWorker::default();
        let mut p = Picker::new(Source::Everything);
        assert!(!p.refresh(&ctx, &mut w, false, &cfg), "nothing typed, nothing waits");
        assert!(!p.results.is_pending(), "and nothing is asked of the worker");
        std::thread::sleep(search_all::DEBOUNCE + std::time::Duration::from_millis(20));
        p.query = "b".into();
        assert!(p.refresh(&ctx, &mut w, false, &cfg), "a key after a pause is asked at once, the list comes from the worker");
        assert!(p.results.is_pending(), "a spinner shows meanwhile");
        wait_for_list(&mut p, &ctx, &mut w, &cfg);
        assert_eq!(p.listed().len(), 2);
        p.query = "ba".into();
        assert!(p.refresh(&ctx, &mut w, false, &cfg), "the next key right after it waits");
        assert!(!p.results.is_pending(), "and asks nothing yet");
        assert_eq!(p.listed().len(), 2, "the old list stays meanwhile");
        p.refresh(&ctx, &mut w, true, &cfg);
        assert!(p.results.is_pending(), "Enter asks for the list of what is typed");
        wait_for_list(&mut p, &ctx, &mut w, &cfg);
        assert_eq!(p.listed().len(), 1);
        p.query = "ban".into();
        assert!(p.refresh(&ctx, &mut w, false, &cfg));
        assert!(!p.results.is_pending());
        std::thread::sleep(search_all::DEBOUNCE + std::time::Duration::from_millis(20));
        p.refresh(&ctx, &mut w, false, &cfg);
        wait_for_list(&mut p, &ctx, &mut w, &cfg);
        assert!(!p.refresh(&ctx, &mut w, false, &cfg), "later the list is made");
        assert_eq!(p.listed()[0].title, "banana");
        // file names are never held back, and never go to the worker
        let mut f = Picker::new(Source::FileNames);
        f.query = "a".into();
        f.refresh(&ctx, &mut w, false, &cfg);
        f.query = "".into();
        assert!(!f.refresh(&ctx, &mut w, false, &cfg));
        assert!(!f.results.is_pending());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: QUERY-014, QUERY-018
    #[test]
    fn emptying_the_box_empties_the_list_at_once_and_drops_the_query_on_its_way() {
        let dir = std::env::temp_dir().join(format!("shale-picker-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("A.md"), "banana\n").unwrap();
        let v = Vault::open(&dir).unwrap();
        let posts = PostIndex::in_memory();
        let ctx = Ctx { vault: &v, mail: None, posts: &posts };
        let cfg = SearchConfig::default();
        let mut w = SearchWorker::default();
        let mut p = Picker::new(Source::Everything);
        p.query = "banana".into();
        p.refresh(&ctx, &mut w, false, &cfg);
        wait_for_list(&mut p, &ctx, &mut w, &cfg);
        assert_eq!(p.listed().len(), 1);
        std::thread::sleep(search_all::DEBOUNCE + std::time::Duration::from_millis(20));
        p.query = "ban".into();
        p.refresh(&ctx, &mut w, false, &cfg);
        assert!(p.results.is_pending());
        p.query = "f:".into();
        assert!(!p.refresh(&ctx, &mut w, false, &cfg), "a prefix alone is nothing to search for: the empty list is made without the worker");
        assert!(!p.results.is_pending() && p.listed().is_empty() && p.results.is_made());
        // (the answer of the query that was on its way is dropped, not listed)
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while w.busy() {
            w.begin_frame(&egui::Context::default());
            assert!(std::time::Instant::now() < end);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        p.refresh(&ctx, &mut w, false, &cfg);
        assert!(p.listed().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: QUERY-008, QUERY-013
    #[test]
    fn a_query_with_a_syntax_error_asks_nothing_and_the_last_valid_list_stays() {
        let dir = std::env::temp_dir().join(format!("shale-picker-invalid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("A.md"), "---\nstatus: doing\n---\nbanana\nberry\n").unwrap();
        let v = Vault::open(&dir).unwrap();
        let posts = PostIndex::in_memory();
        let ctx = Ctx { vault: &v, mail: None, posts: &posts };
        let cfg = SearchConfig::default();
        let mut w = SearchWorker::default();
        let mut p = Picker::new(Source::Everything);
        p.query = "status:doing banana".into();
        p.refresh(&ctx, &mut w, true, &cfg);
        wait_for_list(&mut p, &ctx, &mut w, &cfg);
        assert_eq!(p.listed().len(), 1);
        for bad in ["status:doing banana AND", "(status:doing", "priority:urgent", "due:soon"] {
            p.query = bad.into();
            assert!(!p.refresh(&ctx, &mut w, true, &cfg), "{bad}: nothing waits, not even Enter asks");
            assert!(!p.results.is_pending(), "{bad}: nothing was given to the worker");
            assert_eq!(p.listed().len(), 1, "{bad}: the last valid list stays");
            assert!(p.check.get(bad, cfg.prefixes()).error().is_some());
        }
        // the text is checked once while it stays the same (every frame asks)
        let parses = p.check.parses;
        for _ in 0..5 {
            p.refresh(&ctx, &mut w, false, &cfg);
        }
        assert_eq!(p.check.parses, parses);
        p.query = "status:doing berry".into();
        p.refresh(&ctx, &mut w, true, &cfg);
        wait_for_list(&mut p, &ctx, &mut w, &cfg);
        assert_eq!(p.listed()[0].title, "berry", "a valid text is searched again");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
