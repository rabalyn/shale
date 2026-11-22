//! The search of the left sidebar (Ctrl+Shift+F, NAV-004, NAV-007…NAV-009): the second view of
//! the one search engine of `search_all` (NAV-022). It asks the same question as the `Space f g`
//! picker, with more lines per note ([`Query::sidebar`]), and lists the groups Notes, Mail and
//! Posts with their counts, the lines of a note under the note. The keyboard reaches every hit.

use crate::app::{muted, Action, ShaleApp};
use crate::keys::{Action as KeyAction, Scope};
use crate::search_all::{self, Ctx, Query, Refreshed, Results};
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
    /// the selected hit while the list is browsed with the keyboard (an index into the hits)
    pub sel: Option<usize>,
    /// scroll the selected hit into view on the next frame
    pub(crate) scroll: bool,
    /// focus the search box (and select its text) in the next frame
    pub focus: bool,
    /// the solo of the count buttons (NAV-052); the default selection is in the settings
    pub filter: search_sources::Filter,
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
        let r = ui.add(egui::TextEdit::singleline(&mut self.search.query).id(id).hint_text("Search notes, mail and posts…").desired_width(f32::INFINITY).lock_focus(true));
        if tabbed && !box_focused {
            if let Some(grabbed) = ui.ctx().memory(|m| m.focused()) {
                ui.ctx().memory_mut(|m| m.surrender_focus(grabbed));
            }
        } else if r.changed() || r.has_focus() {
            self.search.sel = None;
        }
        let enter_from_box = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if self.search.query.is_empty() && !r.has_focus() && self.search.sel.is_none() {
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
        let sidebar = |filter: search_sources::Filter| Query::sidebar(query).with_sources(prefixes, filter.shown(self.cfg.search.selection()));
        let ask = sidebar(self.search.filter);
        let refreshed = self.search.results.refresh(query, data.stamp(), typed, enter_from_box, ask.listed(), || search_all::search(&data, &ask));
        if matches!(refreshed, Refreshed::Waits | Refreshed::Relisted) {
            ui.ctx().request_repaint_after(search_all::DEBOUNCE);
        }
        let relisted = refreshed == Refreshed::Relisted;
        let before = self.search.filter;
        let bar = Bar { counts: self.search.results.counts(), limit: search_all::SIDEBAR_GROUP_LIMIT, mail_open: data.mail.is_some(), listed: ask.listed(), prefixed };
        let save = search_sources::bar(ui, &bar, &mut self.search.filter, &mut self.cfg.search);
        if save || self.search.filter != before {
            // the list of the new sources, in the next frame
            ui.ctx().request_repaint();
        }
        let hits = self.search.results.hits();
        if relisted && self.search.sel.is_some() {
            // browsing: the first hit of the other list
            self.search.sel = Some(0);
            self.search.scroll = true;
        }
        let entering = (down_from_box || enter_from_box) && !hits.is_empty();
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
                        let shown: String = h.title.chars().take(120).collect();
                        row(ui, |ui| {
                            ui.add(egui::Label::new(egui::RichText::new(shown).size(12.5).color(theme::palette().muted)).wrap());
                        })
                    }
                    Action::Open { rel, .. } if h.group == search_all::NOTES => {
                        file = Some(rel);
                        ui.add_space(4.0);
                        note_name(ui, v, rel)
                    }
                    Action::OpenMail(_) => row(ui, |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(&h.title).size(12.5).color(theme::palette().text_strong)).truncate());
                        ui.add(egui::Label::new(egui::RichText::new(&h.detail).size(12.0).color(theme::palette().muted)).truncate());
                    }),
                    _ => {
                        let resp = row(ui, |ui| {
                            ui.add(egui::Label::new(egui::RichText::new(&h.detail).size(12.5).color(theme::palette().text_strong)).truncate());
                            ui.add(egui::Label::new(egui::RichText::new(&h.title).size(12.5).color(theme::palette().muted)).wrap());
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

    // spec: NAV-007
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
        frame(&mut app, vec![key(Key::Enter, Modifiers::NONE)]);
        assert_eq!(app.search.sel, Some(0), "Enter in the box selects the first hit");
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

    // spec: NAV-023, NAV-024
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
        assert_eq!(listed(&app).iter().filter(|h| h.1 == "another apple").count(), 1, "{:?}", listed(&app));
        // up to 20 lines of one note, where the picker has three
        std::fs::write(d.join("Many.md"), "pear\n".repeat(25)).unwrap();
        app.vault.as_mut().unwrap().rescan();
        app.search.query = "pear".into();
        pause();
        frame(&mut app, vec![]);
        assert_eq!(listed(&app).len(), 20, "{:?}", listed(&app));
        // an emptied box empties the list at once
        app.search.query.clear();
        frame(&mut app, vec![]);
        assert!(listed(&app).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
