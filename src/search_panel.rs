//! The search of the left sidebar (Ctrl+Shift+F, NAV-004, NAV-007…NAV-009): the second view of
//! the one search engine of `search_all` (NAV-022). It asks the same question as the `Space f g`
//! picker, with more lines per note ([`Query::sidebar`]), and lists the groups Notes, Mail and
//! Posts with their counts, the lines of a note under the note. The keyboard reaches every hit.

use crate::app::{muted, Action, ShaleApp};
use crate::keys::{Action as KeyAction, Scope};
use crate::search_all::{self, Ask, Ctx, Query, Refreshed, Request, Results, Slot, Start};
use crate::search_render::Look;
use crate::search_sources::{self, Bar};
use crate::theme;
use eframe::egui::{self, CornerRadius, Key, Modifiers, Sense, Ui};
use std::path::Path;

/// The search box of the left sidebar, the list it made and the keyboard state of the list.
#[derive(Default)]
pub struct SearchBox {
    pub query: String,
    /// the hits of `query` (NAV-024: made again when the typing has settled or the vault changed)
    results: Results,
    /// the check of the text, made again only when the text changes (QUERY-013)
    check: search_all::query::Cache,
    /// the selected hit while the list is browsed with the keyboard (an index into the hits)
    pub sel: Option<usize>,
    /// scroll the selected hit into view on the next frame
    pub(crate) scroll: bool,
    /// focus the search box (and select its text) in the next frame
    pub focus: bool,
    /// Enter was pressed in the box while the list was on its way: select its first hit when it is in
    enter_wanted: bool,
    /// the solo of the count buttons (NAV-052); the default selection is in the settings
    pub filter: search_sources::Filter,
    /// the filter form under the box (the ⚙ button, QUERY-027) and whether it is open
    pub form: crate::search_form::State,
    pub form_open: bool,
}

impl SearchBox {
    /// The hits now listed (tests).
    #[cfg(test)]
    pub fn listed(&self) -> &[crate::picker::Hit] {
        self.results.hits()
    }
}

/// A clickable block of the list: a rounded box with the muted background of the sidebar's rows.
fn row(ui: &mut Ui, contents: impl FnOnce(&mut Ui)) -> egui::Response {
    let resp = egui::Frame::new().fill(theme::palette().bg_primary).corner_radius(CornerRadius::same(4)).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        contents(ui);
    });
    resp.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The name of a note as the heading of its lines.
fn note_name(ui: &mut Ui, v: &crate::vault::Vault, rel: &Path) -> egui::Response {
    let name = v.file(rel).map(|f| f.display_name()).unwrap_or_default();
    ui.add(egui::Label::new(egui::RichText::new(name).color(theme::palette().text_strong)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The frame around the hit that has the keyboard.
fn mark(ui: &Ui, rect: egui::Rect) {
    ui.painter().rect_stroke(rect.expand(1.0), CornerRadius::same(4), egui::Stroke::new(1.5, theme::palette().accent), egui::StrokeKind::Outside);
}

impl ShaleApp {
    /// Are the lines of notes that the search found drawn rendered now (NAV-058)? The setting
    /// `search.hit_display` says; "follow the editor" is rendered when the Markdown preview is on or
    /// the note in view is in the reading view.
    pub(crate) fn search_rendered(&self) -> bool {
        let reading = self.tabbar.tabs.get(self.tabbar.active).is_some_and(|t| t.reading && matches!(t.view, crate::app::View::Note(_)));
        self.cfg.search.display().rendered(self.cfg.live_preview, reading)
    }

    pub(crate) fn search_panel(&mut self, ui: &mut Ui) {
        ui.label(egui::RichText::new("Search").size(13.0).color(theme::palette().muted));
        let id = egui::Id::unique("search-input");
        if std::mem::take(&mut self.search.focus) {
            // Ctrl+Shift+F: always into the box, with the old search selected (type to replace it,
            // or press an arrow key to change it)
            let mut st = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
            let n = self.search.query.chars().count();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
            st.store(ui.ctx(), id);
            ui.ctx().memory_mut(|m| m.request_focus(id));
        }
        // ↓ in the box goes into the result list (checked before the box sees the key)
        let box_focused = ui.ctx().memory(|m| m.has_focus(id));
        let down_from_box = box_focused && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown));
        // Tab / Shift+Tab choose the source (NAV-055), in the box and while the list is browsed. They are
        // taken before the box is drawn: egui moves the focus on Tab, and the box must not get it.
        // (A selection means the last frame ended with no widget focused, so what has the focus now
        // got it from this Tab: it is taken away again below.)
        let browsing = self.search.sel.is_some();
        let step = if box_focused || browsing { ui.input_mut(|i| (self.keymap.take(i, KeyAction::HitSourcePrev), self.keymap.take(i, KeyAction::HitSourceNext))) } else { (false, false) };
        let tabbed = step.0 || step.1;
        // (the text box underlines what is wrong with the query, QUERY-008)
        let (r, gear) = ui
            .horizontal(|ui| {
                let width = ui.available_width() - 34.0;
                let mut layouter = search_sources::error_layouter(&mut self.search.check, self.cfg.search.prefixes(), None);
                let r = ui.add(egui::TextEdit::singleline(&mut self.search.query).id(id).hint_text("Search notes, mail and posts…").desired_width(width).lock_focus(true).layouter(&mut layouter));
                let gear = ui.add(egui::Button::new("⚙").selected(self.search.form_open)).on_hover_text("Filter form");
                (r, gear)
            })
            .inner;
        // (the key of the form works in the box, and while the form is open)
        let form_key = (box_focused || self.search.form_open) && ui.input_mut(|i| self.keymap.take(i, KeyAction::HitForm));
        if gear.clicked() || form_key {
            self.search.form_open = !self.search.form_open;
            if self.search.form_open {
                self.search.form.focus();
            }
        }
        if tabbed && !box_focused {
            if let Some(grabbed) = ui.ctx().memory(|m| m.focused()) {
                ui.ctx().memory_mut(|m| m.surrender_focus(grabbed));
            }
        } else if r.changed() || r.has_focus() {
            self.search.sel = None;
        }
        if r.changed() {
            self.search.enter_wanted = false;
        }
        let enter_from_box = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if self.search.query.is_empty() && !r.has_focus() && self.search.sel.is_none() && (!self.search.form_open || ui.memory(|m| m.focused().is_none())) {
            r.request_focus();
        }
        ui.add_space(6.0);
        let Some(v) = &self.vault else { return };
        let data = Ctx { vault: v, mail: self.mail.index(), posts: &self.social.index };
        // the same engine as the picker; Enter in the box makes the list of what is typed now, and
        // an emptied box empties the list at once
        let typed = !self.search.query.trim().is_empty();
        let prefixes = self.cfg.search.prefixes();
        let prefixed = prefixes.parse(&self.search.query).prefix.is_some();
        let available = Bar::available(data.mail.is_some());
        if !prefixed && tabbed {
            self.search.filter.step(step.1, &available);
        }
        let query = &self.search.query;
        let checked = self.search.check.get(query, prefixes);
        let ask = Query::sidebar(query).with_sources(prefixes, self.search.filter.shown(self.cfg.search.selection())).with_checked(checked);
        let worker = &mut self.search_worker;
        let listed = ask.listed();
        let by = Ask { slot: Slot::SIDEBAR, text: query, stamp: data.stamp(), debounced: typed, now: enter_from_box, listed: ask.listed(), blank: ask.is_blank(), invalid: checked.error().is_some() };
        let refreshed = self.search.results.refresh(worker, &by, |w| Start::Queued(w.submit(Slot::SIDEBAR, Request::new(&data, &ask))));
        if matches!(refreshed, Refreshed::Waits | Refreshed::Relisted) {
            ui.ctx().request_repaint_after(search_all::DEBOUNCE);
        }
        let relisted = refreshed == Refreshed::Relisted;
        let before = self.search.filter;
        let bar = Bar { counts: self.search.results.counts(), limit: search_all::SIDEBAR_GROUP_LIMIT, mail_open: data.mail.is_some(), listed: ask.listed(), prefixed, searching: self.search.results.is_pending() };
        let save = search_sources::bar(ui, &bar, &mut self.search.filter, &mut self.cfg.search);
        let skipped: Vec<String> = self.search.results.skipped(ask.listed()).map(search_all::Skipped::message).collect();
        search_sources::notices(ui, checked.error(), &skipped, self.search.results.stopped_early());
        if save || self.search.filter != before {
            // the list of the new sources, in the next frame
            ui.ctx().request_repaint();
        }
        let mut save = save;
        if self.search.form_open {
            let count = self.search.results.counts().map(|c| c.matches_text(listed, search_all::SIDEBAR_GROUP_LIMIT));
            let opts = crate::search_form::Opts { vault: v, prefixes, host_box: true, count: count.as_deref() };
            let changed = crate::search_form::show(ui, &mut self.search.form, &mut self.search.query, &mut self.search.check, &mut self.cfg.search.advanced, None, &opts);
            save |= changed.advanced;
            if changed.query {
                // what the form wrote is searched like what was typed: when the typing has settled
                self.search.enter_wanted = false;
                self.search.sel = None;
                ui.ctx().request_repaint();
            }
        }
        let hits = self.search.results.hits();
        if relisted && self.search.sel.is_some() {
            // browsing: the first hit of the other list
            self.search.sel = Some(0);
            self.search.scroll = true;
        }
        // Enter in the box browses the list of what is typed: when it is still on its way from the
        // worker, the first hit is selected when it arrives
        self.search.enter_wanted |= enter_from_box;
        let enter_ready = self.search.enter_wanted && !self.search.results.is_pending();
        self.search.enter_wanted &= !enter_ready;
        let entering = (down_from_box || enter_ready) && !hits.is_empty();
        if entering {
            self.search.sel = Some(0);
            self.search.scroll = true;
            ui.ctx().memory_mut(|m| m.surrender_focus(id));
        }
        // browsing: only while no text field has the keyboard (so j and k never type)
        let mut open_sel = false;
        // (not in the frame the list was entered: the Enter that got us here must not open it)
        if let Some(sel) = self.search.sel.filter(|_| !entering) {
            let free = ui.ctx().memory(|m| m.focused().is_none());
            if !free || hits.is_empty() {
                self.search.sel = None;
            } else {
                let sel = sel.min(hits.len() - 1);
                let (action, enter, back) = ui.input_mut(|i| (self.keymap.poll(i, Scope::SearchResults), i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape) | i.consume_key(Modifiers::NONE, Key::Slash)));
                let new = match action {
                    Some(KeyAction::HitNextFile) => search_all::next_block(hits, sel),
                    Some(KeyAction::HitPrevFile) => search_all::prev_block(hits, sel),
                    Some(KeyAction::HitNext) => (sel + 1).min(hits.len() - 1),
                    Some(KeyAction::HitPrev) => sel.saturating_sub(1),
                    _ => sel,
                };
                self.search.sel = Some(new);
                self.search.scroll |= new != sel;
                if back {
                    self.search.sel = None;
                    ui.ctx().memory_mut(|m| m.request_focus(id));
                } else if enter {
                    open_sel = true;
                }
            }
        }
        if typed {
            let source_keys = [(KeyAction::HitSourceNext, "next"), (KeyAction::HitSourcePrev, "previous")].into_iter().filter_map(|(a, what)| self.keymap.hint(a).map(|k| format!("{k} {what} source"))).collect::<Vec<_>>().join(", ");
            let source_keys = if source_keys.is_empty() { String::new() } else { format!(", {source_keys}") };
            let hint = if self.search.sel.is_some() {
                format!("j/k hit to hit, Ctrl+J/K next/previous file or group{source_keys}, Enter opens, Esc back")
            } else if hits.is_empty() {
                if self.search.results.is_made() {
                    "Nothing found".to_string()
                } else {
                    String::new()
                }
            } else {
                "↓ to browse".to_string()
            };
            if !hint.is_empty() {
                ui.label(muted(hint).size(12.0));
            }
        }
        let rendered = self.search_rendered();
        let selected = self.search.sel;
        if open_sel {
            if let Some(hit) = selected.and_then(|i| hits.get(i)) {
                // an open note takes the keyboard into its editor, as after a pick in the picker
                self.actions.push(hit.action.clone());
            }
            self.search.sel = None;
        }
        let scroll = std::mem::take(&mut self.search.scroll);
        let headers = search_all::group_headers(hits, search_all::SIDEBAR_GROUP_LIMIT);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let (mut group, mut file) = ("", None::<&Path>);
            self.social.hit_rects.clear();
            let mut posts = 0;
            for (i, h) in hits.iter().enumerate() {
                if h.group != group {
                    group = h.group;
                    file = None;
                    if let Some((_, text)) = headers.iter().find(|(g, _)| *g == group) {
                        ui.label(muted(text).size(12.0));
                    }
                }
                let resp = match &h.action {
                    // a note: its name once, its found lines under it; a name-only hit is the name
                    Action::GotoMatch { rel, .. } if h.group == search_all::NOTES => {
                        if file != Some(rel.as_path()) {
                            file = Some(rel);
                            ui.add_space(4.0);
                            if note_name(ui, v, rel).clicked() {
                                self.actions.push(Action::Open { rel: rel.clone(), new_tab: false });
                            }
                        }
                        // (the title is at most search_render::MAX_SHOWN_CHARS long)
                        let job = h.job(Look { rendered, size: 12.5, raw_colour: theme::palette().muted });
                        row(ui, |ui| {
                            let shown = ui.add(egui::Label::new(job.clone()).wrap());
                            crate::search_render::paint_task_boxes(ui, &job, |_| shown.rect.min);
                        })
                    }
                    Action::Open { rel, .. } if h.group == search_all::NOTES => {
                        file = Some(rel);
                        ui.add_space(4.0);
                        note_name(ui, v, rel)
                    }
                    Action::OpenMail(_) => row(ui, |ui| {
                        ui.add(egui::Label::new(h.job(Look { rendered: false, size: 12.5, raw_colour: theme::palette().text_strong })).truncate());
                        ui.add(egui::Label::new(egui::RichText::new(&h.detail).size(12.0).color(theme::palette().muted)).truncate());
                    }),
                    _ => {
                        let resp = row(ui, |ui| {
                            ui.add(egui::Label::new(egui::RichText::new(&h.detail).size(12.5).color(theme::palette().text_strong)).truncate());
                            ui.add(egui::Label::new(h.job(Look { rendered: false, size: 12.5, raw_colour: theme::palette().muted })).wrap());
                        });
                        // the rows of posts, for the tests that click them
                        self.social.hit_rects.push((posts, resp.rect));
                        posts += 1;
                        resp
                    }
                };
                if selected == Some(i) {
                    mark(ui, resp.rect);
                    if scroll {
                        resp.scroll_to_me(Some(egui::Align::Center));
                    }
                }
                if resp.clicked() {
                    self.actions.push(h.action.clone());
                }
                ui.add_space(if h.group == search_all::NOTES { 2.0 } else { 4.0 });
            }
        });
        if save {
            // Shift+click on a button changed the default selection
            self.save_config();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Options;
    use eframe::egui::{self, Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2};
    use std::path::PathBuf;

    fn key(k: Key, modifiers: Modifiers) -> Event {
        Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers }
    }
    /// Press a key (also sending the letter as typed text, as a real keyboard does).
    fn press(frame: &mut dyn FnMut(&mut ShaleApp, Vec<Event>), app: &mut ShaleApp, k: Key, m: Modifiers) -> Option<usize> {
        let typed = if m.ctrl || !matches!(k, Key::J | Key::K) { vec![] } else { vec![Event::Text(format!("{k:?}").to_lowercase())] };
        frame(app, [vec![key(k, m)], typed].concat());
        app.search.sel
    }
    /// Wait until the typing has settled (NAV-024): the next frame makes the list.
    fn pause() {
        std::thread::sleep(search_all::DEBOUNCE + std::time::Duration::from_millis(30));
    }
    /// Frames until the worker has answered (QUERY-014): the list of the sidebar comes from another thread.
    fn wait(frame: &mut dyn FnMut(&mut ShaleApp, Vec<Event>), app: &mut ShaleApp) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.search.results.is_pending() {
            assert!(std::time::Instant::now() < end, "the worker never answered");
            std::thread::sleep(std::time::Duration::from_millis(2));
            frame(app, vec![]);
        }
    }
    const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

    /// A vault where "apple" is found: twice in A, by name only in "apple notes", once in B.
    fn browse_app(tag: &str) -> (ShaleApp, PathBuf) {
        let d = std::env::temp_dir().join(format!("shale-search-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("A.md"), "first apple\nno\nsecond Apple here\n").unwrap();
        std::fs::write(d.join("apple notes.md"), "nothing to see\n").unwrap();
        std::fs::write(d.join("B.md"), "  Ünïcode ÄPFEL apple\n").unwrap();
        (ShaleApp::build(Options { screenshot: None, vault: Some(d.clone()), view: None, keys: None }, false), d)
    }

    // spec: NAV-007
    #[test]
    fn the_search_box_says_what_it_searches() {
        let (mut app, d) = browse_app("hint");
        let ctx = egui::Context::default();
        let mut texts = vec![];
        for i in 0..2 {
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))), time: Some(0.05 * (i + 1) as f64), ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.search_panel(ui));
            });
            out.textures_delta.clear();
            texts = out.shapes.iter().filter_map(|c| if let egui::epaint::Shape::Text(t) = &c.shape { Some(t.galley.text().to_string()) } else { None }).collect();
        }
        assert!(texts.iter().any(|t| t == "Search notes, mail and posts…"), "{texts:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: NAV-007, NAV-008, NAV-009
    #[test]
    fn search_results_are_browsed_with_j_k_and_ctrl_j_k() {
        let (mut app, d) = browse_app("browse");
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut frame = |app: &mut ShaleApp, events: Vec<Event>| {
            t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))), time: Some(t), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.search_panel(ui));
            });
            out.textures_delta.clear();
        };
        let id = egui::Id::unique("search-input");
        frame(&mut app, vec![]);
        frame(&mut app, vec![Event::Text("apple".into())]);
        pause();
        frame(&mut app, vec![]);
        wait(&mut frame, &mut app);
        assert_eq!(app.search.query, "apple");
        assert_eq!(app.search.sel, None, "typing in the box is not browsing");
        // ↓ goes into the list: stops are A:0, A:2, "apple notes" (name only), B:0
        frame(&mut app, vec![key(Key::ArrowDown, Modifiers::NONE)]);
        assert_eq!(app.search.sel, Some(0));
        assert!(!ctx.memory(|m| m.has_focus(id)), "the box gives up the keyboard");
        assert_eq!(press(&mut frame, &mut app, Key::J, Modifiers::NONE), Some(1));
        assert_eq!(press(&mut frame, &mut app, Key::J, Modifiers::NONE), Some(2));
        assert_eq!(press(&mut frame, &mut app, Key::J, Modifiers::NONE), Some(3));
        assert_eq!(press(&mut frame, &mut app, Key::J, Modifiers::NONE), Some(3), "stops at the end");
        assert_eq!(press(&mut frame, &mut app, Key::K, Modifiers::NONE), Some(2));
        assert_eq!(app.search.query, "apple", "j and k did not type into the box");
        // Ctrl+J / Ctrl+K: first stop of the next / previous file
        assert_eq!(press(&mut frame, &mut app, Key::K, CTRL), Some(0), "back to A");
        assert_eq!(press(&mut frame, &mut app, Key::J, CTRL), Some(2), "on to \"apple notes\"");
        assert_eq!(press(&mut frame, &mut app, Key::J, CTRL), Some(3), "on to B");
        assert_eq!(press(&mut frame, &mut app, Key::J, CTRL), Some(3), "no file after B");
        assert_eq!(press(&mut frame, &mut app, Key::ArrowUp, Modifiers::NONE), Some(2), "arrow keys work too");
        assert_eq!(press(&mut frame, &mut app, Key::K, Modifiers::NONE), Some(1));
        // Enter opens the second hit in A at "Apple"
        app.actions.clear();
        press(&mut frame, &mut app, Key::Enter, Modifiers::NONE);
        assert_eq!(app.search.sel, None);
        let Some(Action::GotoMatch { rel, line, col }) = app.actions.pop() else { panic!("Enter opens the hit") };
        assert_eq!((rel.as_path(), line, col), (Path::new("A.md"), 2, 7));
        app.goto_match(&rel, line, col);
        assert_eq!(app.current_path(), Some(PathBuf::from("A.md")));
        let text = &app.notes[Path::new("A.md")].text;
        let cursor = app.tabbar.tabs[app.tabbar.active].editor.pending_cursor.expect("cursor placed (and the editor focused)");
        assert!(text.chars().skip(cursor).collect::<String>().starts_with("Apple here"));
        // a name-only stop opens the file; non-ASCII text before the match is counted right
        frame(&mut app, vec![]);
        ctx.memory_mut(|m| m.request_focus(id));
        frame(&mut app, vec![]);
        frame(&mut app, vec![key(Key::ArrowDown, Modifiers::NONE)]);
        press(&mut frame, &mut app, Key::J, CTRL);
        app.actions.clear();
        press(&mut frame, &mut app, Key::Enter, Modifiers::NONE);
        assert!(matches!(app.actions.pop(), Some(Action::Open { rel, .. }) if rel == Path::new("apple notes.md")));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: NAV-007, NAV-009
    #[test]
    fn enter_in_the_box_selects_and_esc_goes_back() {
        let (mut app, d) = browse_app("enter");
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut frame = |app: &mut ShaleApp, events: Vec<Event>| {
            t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))), time: Some(t), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.search_panel(ui));
            });
            out.textures_delta.clear();
        };
        let id = egui::Id::unique("search-input");
        frame(&mut app, vec![]);
        frame(&mut app, vec![Event::Text("äpfel".into())]);
        wait(&mut frame, &mut app);
        frame(&mut app, vec![key(Key::Enter, Modifiers::NONE)]);
        wait(&mut frame, &mut app);
        assert_eq!(app.search.sel, Some(0), "Enter in the box selects the first hit, when the list of what is typed is in");
        assert!(app.actions.is_empty(), "…and does not open it yet");
        frame(&mut app, vec![key(Key::Escape, Modifiers::NONE)]);
        frame(&mut app, vec![]);
        assert_eq!(app.search.sel, None);
        assert!(ctx.memory(|m| m.has_focus(id)), "Esc goes back into the box");
        // the match position counts bytes of the original line (Ü, ï are two bytes each)
        let v = app.vault.as_ref().unwrap();
        let hits = search_all::note_hits(v, &Query::sidebar("äpfel"));
        assert_eq!(hits.len(), 1);
        let Action::GotoMatch { col, .. } = hits[0].action else { panic!("a line of a note") };
        assert_eq!(&"  Ünïcode ÄPFEL apple"[col..], "ÄPFEL apple");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: NAV-004
    #[test]
    fn the_search_shortcut_always_focuses_and_selects_the_old_search() {
        let d = std::env::temp_dir().join(format!("shale-search-focus-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        crate::vault::write_sample_vault(&d).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(d.clone()), view: None, keys: None }, false);
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut frame = |app: &mut ShaleApp, events: Vec<Event>| {
            t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))), time: Some(t), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.search_panel(ui));
            });
            out.textures_delta.clear();
        };
        app.search.query = "garden".into();
        frame(&mut app, vec![]);
        // focus elsewhere (as after working in a note), then the shortcut
        ctx.memory_mut(|m| m.surrender_focus(egui::Id::unique("search-input")));
        frame(&mut app, vec![]);
        let id = egui::Id::unique("search-input");
        assert!(!ctx.memory(|m| m.has_focus(id)));
        app.search.focus = true;
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        assert!(ctx.memory(|m| m.has_focus(id)), "the box has focus although it isn't empty");
        let range = egui::text_edit::TextEditState::load(&ctx, id).and_then(|s| s.cursor.char_range()).unwrap();
        assert_eq!((range.primary.index.0.min(range.secondary.index.0), range.primary.index.0.max(range.secondary.index.0)), (0, 6), "the old search is selected");
        // typing replaces it
        frame(&mut app, vec![Event::Text("compost".into())]);
        assert_eq!(app.search.query, "compost");
        // an arrow key keeps it and moves the cursor
        app.search.focus = true;
        frame(&mut app, vec![]);
        frame(&mut app, vec![Event::Key { key: Key::ArrowRight, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        frame(&mut app, vec![Event::Text("s".into())]);
        assert_eq!(app.search.query, "composts");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The sidebar with a frame of its own: the hits it lists as (group, title).
    fn listed(app: &ShaleApp) -> Vec<(&'static str, String)> {
        app.search.listed().iter().map(|h| (h.group, h.title.clone())).collect()
    }

    // spec: NAV-023, NAV-024, PERF-004, QUERY-014
    #[test]
    fn the_sidebar_search_takes_wildcards_waits_for_the_typing_and_follows_the_vault() {
        let (mut app, d) = browse_app("engine");
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut frame = |app: &mut ShaleApp, events: Vec<Event>| {
            t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))), time: Some(t), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.search_panel(ui));
            });
            out.textures_delta.clear();
        };
        frame(&mut app, vec![]);
        // a key right after the first frame waits; the old (empty) list stays meanwhile
        frame(&mut app, vec![Event::Text("ap?le".into())]);
        frame(&mut app, vec![]);
        assert!(listed(&app).is_empty(), "the list is not made while the typing may go on");
        pause();
        frame(&mut app, vec![]);
        assert!(app.search.results.is_pending(), "the list is on its way from the worker (QUERY-018)");
        assert!(listed(&app).is_empty(), "and the old one stays until it arrives");
        wait(&mut frame, &mut app);
        let titles: Vec<String> = listed(&app).into_iter().map(|h| h.1).collect();
        assert_eq!(titles, ["first apple", "second Apple here", "apple notes", "Ünïcode ÄPFEL apple"], "a wildcard finds the lines, in the sidebar too, and the name");
        // no re-search for the same query and state: the list is the same object
        let before = app.search.listed().as_ptr();
        frame(&mut app, vec![]);
        assert_eq!(app.search.listed().as_ptr(), before);
        // a note edited while the search is shown shows up (after the debounce)
        app.vault.as_mut().unwrap().set_note_text(Path::new("B.md"), "# B\nanother apple\n");
        pause();
        frame(&mut app, vec![]);
        wait(&mut frame, &mut app);
        assert_eq!(listed(&app).iter().filter(|h| h.1 == "another apple").count(), 1, "{:?}", listed(&app));
        // up to 20 lines of one note, where the picker has three
        std::fs::write(d.join("Many.md"), "pear\n".repeat(25)).unwrap();
        app.vault.as_mut().unwrap().rescan();
        app.search.query = "pear".into();
        pause();
        frame(&mut app, vec![]);
        wait(&mut frame, &mut app);
        assert_eq!(listed(&app).len(), 20, "{:?}", listed(&app));
        // an emptied box empties the list at once
        app.search.query.clear();
        frame(&mut app, vec![]);
        assert!(listed(&app).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: QUERY-008, QUERY-013
    #[test]
    fn the_sidebar_shows_a_syntax_error_and_keeps_the_last_valid_list() {
        let (mut app, d) = browse_app("invalid");
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut frame = |app: &mut ShaleApp, events: Vec<Event>| {
            t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))), time: Some(t), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.search_panel(ui));
            });
            out.textures_delta.clear();
        };
        frame(&mut app, vec![]);
        app.search.query = "apple".into();
        pause();
        frame(&mut app, vec![]);
        wait(&mut frame, &mut app);
        let before = listed(&app);
        assert_eq!(before.len(), 4);
        app.search.query = "apple AND (".into();
        pause();
        frame(&mut app, vec![]);
        assert!(!app.search.results.is_pending() && !app.search_worker.busy(), "nothing is asked of the worker");
        assert_eq!(listed(&app), before, "the last valid list stays");
        let prefixes = app.cfg.search.prefixes();
        assert!(app.search.check.get("apple AND (", prefixes).error().is_some(), "the error is kept with the text");
        let parses = app.search.check.parses;
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        assert_eq!(app.search.check.parses, parses, "QUERY-013: frames do not parse again");
        // a text that is valid again is searched
        app.search.query = "apple NOT second".into();
        pause();
        frame(&mut app, vec![]);
        wait(&mut frame, &mut app);
        assert_eq!(listed(&app).len(), 2, "A has a line with 'second', so the note is out: {:?}", listed(&app));
        // a field selects notes: the one with the apple on its first line only (its name has the word, too)
        app.search.query = "path:B apple".into();
        pause();
        frame(&mut app, vec![]);
        wait(&mut frame, &mut app);
        assert_eq!(listed(&app), [("Notes", "Ünïcode ÄPFEL apple".to_string())]);
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: QUERY-018
    #[test]
    fn the_search_box_keeps_drawing_with_the_last_list_while_a_search_runs() {
        fn hold(_: &mut search_all::worker::Conns, _: &search_all::Request, cancel: &std::sync::atomic::AtomicBool) -> Option<search_all::Found> {
            let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !cancel.load(std::sync::atomic::Ordering::Relaxed) && std::time::Instant::now() < end {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            None
        }
        let (mut app, d) = browse_app("keeps-drawing");
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut frame = |app: &mut ShaleApp, events: Vec<Event>| {
            t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))), time: Some(t), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.search_panel(ui));
            });
            out.textures_delta.clear();
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![Event::Text("apple".into())]);
        pause();
        frame(&mut app, vec![]);
        wait(&mut frame, &mut app);
        assert_eq!(listed(&app).len(), 4);
        // the next query never ends: the window goes on, with the last list and a spinner
        app.search_worker = search_all::SearchWorker::with_runner(hold);
        pause();
        app.search.query = "apple pie".into();
        frame(&mut app, vec![]);
        assert!(app.search.results.is_pending() && app.search_worker.busy());
        let began = std::time::Instant::now();
        for _ in 0..20 {
            frame(&mut app, vec![]);
        }
        assert!(began.elapsed() < std::time::Duration::from_secs(5), "typing and drawing are not blocked");
        assert!(app.search.results.is_pending(), "still searching");
        assert_eq!(listed(&app).len(), 4, "the last valid list stays");
        // a newer query gives the old one up
        pause();
        app.search.query = "apple".into();
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        assert!(app.search.results.is_pending());
        drop(app);
        let _ = std::fs::remove_dir_all(&d);
    }
}
