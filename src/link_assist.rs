//! Link helpers for any text box: `[[` suggests notes and drawings, `[[p ` suggests people,
//! `[!` / `[[!` opens the file picker. Used by the note editor and by text in drawings.
//!
//! A text box that wants the helpers calls, each frame:
//! 1. [`LinkAssist::take_keys`] before the text box is drawn (the popups need the arrow keys,
//!    Enter, Tab and Esc before the text box sees them),
//! 2. [`LinkAssist::arm`] right before the text box (after any other change to the text),
//! 3. [`LinkAssist::after`] with the text box's output (popups, picking, safety net).

use crate::editor::EditorAction;
use crate::markdown;
use crate::media::intake::{Choice, OutsideFiles};
use crate::theme;
use crate::vault::Vault;
use eframe::egui::{self, Key, Modifiers, Ui, Vec2};
use egui::text::{CCursor, CCursorRange};
use std::path::Path;

/// State of the `[!path` file picker.
#[derive(Default)]
pub(crate) struct MediaPicker {
    pub(crate) active: bool,
    pub(crate) sel: usize,
    pub(crate) items: Vec<crate::media::Entry>,
    /// (folder, partial) the items were listed for
    key: Option<(std::path::PathBuf, String)>,
    /// Esc pressed for the picker starting at this byte
    dismissed: Option<usize>,
    /// cursor (byte) when the picker was last shown
    cursor: Option<usize>,
    /// the popup was drawn in the last frame
    shown: bool,
    /// byte where the `[!` of the shown picker starts
    start: Option<usize>,
    error: Option<String>,
    /// what to do with files from outside the vault for this pick, set with Alt+Shift+L (MEDIA-019);
    /// None: what the setting says
    pub(crate) how: Option<Choice>,
    /// a media file from outside the vault was picked and the setting is Ask: the picker asks
    /// whether to copy or link it
    pub(crate) asking: Option<std::path::PathBuf>,
}

/// What [`LinkAssist::take_keys`] did this frame.
#[derive(Default)]
pub struct Keys {
    /// the text changed (a pick or a Tab completion)
    pub changed: bool,
    pub action: Option<EditorAction>,
    /// the file picker took a key
    pub handled: bool,
    /// the file picker is open: Enter / Tab must not reach other helpers of the text box
    pub picker_open: bool,
    /// an entry of the `[[` list is being picked
    pub picking: bool,
}

pub struct LinkAssist {
    /// id of the text box the helpers work on
    pub id: egui::Id,
    /// folder of the contact notes (`[[p` lists these; set from the app's settings)
    pub people_dir: String,
    /// the setting for numbered headings (CFG-042): the heading list shows the numbers
    pub numbered_headings: bool,
    /// the setting for files from outside the vault (MEDIA-019): what a pick does with them
    pub outside: OutsideFiles,
    pub(crate) ac_active: bool,
    /// byte where the `[[` of the open suggestions starts
    ac_start: Option<usize>,
    /// Esc was pressed for the suggestions starting at this byte
    ac_dismissed: Option<usize>,
    pub(crate) ac_sel: usize,
    ac_items: Vec<(String, bool)>,
    /// what the list shows for each item (the people list shows names, not link texts)
    pub(crate) ac_labels: Vec<String>,
    /// the open list is the `[[p` people list
    ac_people: bool,
    pub(crate) media: MediaPicker,
    /// this frame: the `[[` entry picked by keyboard
    ac_pick: Option<usize>,
    /// this frame: the text going into the text box, and the picker state then
    text_before: String,
    picker_before: Option<(usize, usize, String)>,
}

/// What was typed after a `[!` in the text: the path so far (`typed`) sits at `start..cursor` (byte
/// offsets), to be completed against the vault and the home folder.
#[derive(Clone, Copy)]
struct MediaTyped<'a> {
    vault: &'a Vault,
    home: &'a Path,
    start: usize,
    cursor: usize,
    typed: &'a str,
}

impl LinkAssist {
    pub fn new(id: egui::Id) -> Self {
        LinkAssist {
            id,
            people_dir: crate::import::people::DEFAULT_DIR.into(),
            numbered_headings: true,
            outside: OutsideFiles::default(),
            ac_active: false,
            ac_start: None,
            ac_dismissed: None,
            ac_sel: 0,
            ac_items: vec![],
            ac_labels: vec![],
            ac_people: false,
            media: MediaPicker::default(),
            ac_pick: None,
            text_before: String::new(),
            picker_before: None,
        }
    }

    /// Is the `[!` file picker popup on screen?
    pub fn picker_visible(&self) -> bool {
        self.media.shown
    }

    /// Is a suggestion list or the file picker open?
    pub fn is_open(&self) -> bool {
        self.media.active || self.ac_active
    }

    /// Short description of the picker state (testing aid).
    pub fn debug_state(&self) -> String {
        format!(
            "picker active={} shown={} sel={} items={} dismissed={:?} start={:?} | suggestions active={} (input method is switched off while either is open)",
            self.media.active,
            self.media.shown,
            self.media.sel,
            self.media.items.len(),
            self.media.dismissed,
            self.media.start,
            self.ac_active
        )
    }

    /// Where the cursor is in a `[!path` that is being typed: (cursor byte, byte where the `[!` starts, the path so far).
    fn media_context(&self, ui: &Ui, text: &str) -> Option<(usize, usize, String)> {
        let cur = egui::text_edit::TextEditState::load(ui.ctx(), self.id).and_then(|st| st.cursor.char_range()).filter(|r| r.is_empty()).map(|r| markdown::char_to_byte(text, r.primary.index.0)).or(self.media.cursor)?;
        crate::media::media_query(text, cur).map(|(start, typed)| (cur, start, typed))
    }

    fn set_cursor_byte(&self, ui: &Ui, text: &str, byte: usize) {
        let c = markdown::byte_to_char(text, byte);
        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), self.id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(c))));
        state.store(ui.ctx(), self.id);
    }

    /// Before the text box: take the keys the open popups need.
    pub fn take_keys(&mut self, ui: &mut Ui, text: &mut String, vault: &Vault) -> Keys {
        let mut out = Keys::default();
        self.ac_pick = None;
        if self.ac_active && !self.ac_items.is_empty() {
            let n = self.ac_items.len();
            ui.input_mut(|i| {
                if down_key(i) {
                    self.ac_sel = (self.ac_sel + 1) % n;
                }
                if up_key(i) {
                    self.ac_sel = (self.ac_sel + n - 1) % n;
                }
                if i.consume_key(Modifiers::NONE, Key::Enter) || i.consume_key(Modifiers::NONE, Key::Tab) {
                    self.ac_pick = Some(self.ac_sel.min(n - 1));
                }
                if i.consume_key(Modifiers::NONE, Key::Escape) {
                    self.ac_active = false;
                    self.ac_dismissed = self.ac_start;
                }
            });
        }
        out.picking = self.ac_pick.is_some();

        // `[!path` file picker: Tab completes, arrows choose, Enter picks, Esc closes.
        let home = dirs::home_dir().unwrap_or_default();
        // the question "copy or link?" for a file that was just picked: C, L, Esc and nothing else
        if let Some(src) = self.media.asking.clone() {
            let (copy, link, esc) = ui.input_mut(|i| {
                let keys = (i.consume_key(Modifiers::NONE, Key::C) || i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::L), i.consume_key(Modifiers::NONE, Key::Escape));
                // what else is typed does not reach the text
                i.events.retain(|e| match e {
                    egui::Event::Text(_) | egui::Event::Paste(_) | egui::Event::Ime(_) => false,
                    egui::Event::Key { key, modifiers, pressed: true, .. } => {
                        modifiers.any() || !matches!(key, Key::Tab | Key::Backspace | Key::Delete | Key::ArrowDown | Key::ArrowUp | Key::ArrowLeft | Key::ArrowRight | Key::Home | Key::End)
                    }
                    _ => true,
                });
                keys
            });
            out.handled = true;
            out.picker_open = true;
            let how = if copy {
                Some(Choice::Copy)
            } else if link {
                Some(Choice::Link)
            } else {
                None
            };
            if esc {
                self.media.asking = None;
            } else if let Some(how) = how {
                match self.media_context(ui, text) {
                    Some((cb, start, typed)) => {
                        let (c, a) = self.media_finish(ui, text, &MediaTyped { vault, home: &home, start, cursor: cb, typed: &typed }, &src, how);
                        out.changed |= c;
                        out.action = a;
                    }
                    None => self.media.asking = None,
                }
            }
            return out;
        }
        // Esc closes the picker whenever it was on screen, whatever state the cursor is in.
        if self.media.shown && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.media.dismissed = self.media.start;
            self.media.active = false;
            self.media.shown = false;
            out.handled = true;
        }
        if self.media.active {
            if let Some((cb, start, typed)) = self.media_context(ui, text) {
                let n = self.media.items.len();
                // Alt+Shift+L: for this pick, files from outside the vault are linked where they are, or copied (MEDIA-019)
                let toggle = ui.input_mut(|i| {
                    let pressed = i.consume_key(Modifiers::ALT | Modifiers::SHIFT, Key::L);
                    if pressed {
                        // Option+Shift+L is a letter on a Mac: not typed
                        i.events.retain(|e| !matches!(e, egui::Event::Text(_)));
                    }
                    pressed
                });
                if toggle {
                    self.media.how = Some(self.media.how.or(self.outside.choice()).map_or(Choice::Link, Choice::toggled));
                }
                let (down, up, tab, enter, esc) =
                    ui.input_mut(|i| (n > 0 && down_key(i), n > 0 && up_key(i), i.consume_key(Modifiers::NONE, Key::Tab), n > 0 && i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape)));
                if down {
                    self.media.sel = (self.media.sel + 1) % n;
                }
                if up {
                    self.media.sel = (self.media.sel + n - 1) % n;
                }
                if esc {
                    self.media.dismissed = Some(start);
                    self.media.active = false;
                }
                if tab {
                    out.changed |= self.media_tab(ui, text, &MediaTyped { vault, home: &home, start, cursor: cb, typed: &typed });
                }
                if enter {
                    if let Some(e) = self.media.items.get(self.media.sel.min(n - 1)).cloned() {
                        let (c, a) = self.media_accept(ui, text, &MediaTyped { vault, home: &home, start, cursor: cb, typed: &typed }, &e);
                        out.changed |= c;
                        if a.is_some() {
                            out.action = a;
                        }
                    }
                }
                // only count as handled when the picker actually took a key
                out.handled = down || up || tab || enter || esc || toggle;
                out.picker_open = true;
            } else {
                self.media.active = false;
            }
        }
        out
    }

    /// Right before the text box is drawn: remember the text and picker state going in, for the
    /// safety net in [`LinkAssist::after`]. `handled` = [`Keys::handled`] of this frame.
    pub fn arm(&mut self, text: &str, handled: bool) {
        self.text_before = text.to_string();
        self.picker_before = if self.media.active && !handled { self.media.cursor.and_then(|cb| crate::media::media_query(text, cb).map(|(st, typed)| (cb, st, typed))) } else { None };
    }

    /// After the text box: the `[[` list and the `[!` picker. `typing` = keys type text (not
    /// vim's normal mode); `handled` = [`Keys::handled`] of this frame.
    pub fn after(&mut self, ui: &mut Ui, text: &mut String, vault: &Vault, output: &egui::text_edit::TextEditOutput, typing: bool, handled: bool) -> (bool, Option<EditorAction>) {
        let mut changed = false;
        let mut action = None;
        let resp = &output.response.response;
        let home = dirs::home_dir().unwrap_or_default();
        let mut media_handled = handled;
        if media_handled && self.media.active && !resp.has_focus() {
            ui.ctx().memory_mut(|m| m.request_focus(self.id));
            ui.ctx().request_repaint();
        }

        // --- [[ suggestions
        // A press on the open list takes the keyboard from the text box (egui gives up the focus
        // on a press elsewhere) and the list is only shown while the text box has it: keep the
        // list (at the cursor the text box had) and give the keyboard back, so the click can pick.
        let on_list = self.ac_active && ui.input(|i| i.pointer.interact_pos()).zip(ui.ctx().memory(|m| m.area_rect(self.id.with("ac")))).is_some_and(|(p, r)| r.contains(p));
        if on_list && !resp.has_focus() {
            ui.ctx().memory_mut(|m| m.request_focus(self.id));
            ui.ctx().request_repaint();
        }
        let cursor = output
            .cursor_range
            .filter(|r| r.is_empty())
            .map(|r| r.primary.index.0)
            .or_else(|| on_list.then(|| egui::text_edit::TextEditState::load(ui.ctx(), self.id).and_then(|st| st.cursor.char_range()).filter(|r| r.is_empty()).map(|r| r.primary.index.0)).flatten());
        let query = if (resp.has_focus() || on_list) && typing { cursor.and_then(|c| link_query(text, c)) } else { None };
        // a dismissed `[[` stays closed until that `[[` is gone from the text
        if let Some(d) = self.ac_dismissed {
            if !text.get(d.saturating_sub(2)..).map(|t| t.starts_with("[[")).unwrap_or(false) {
                self.ac_dismissed = None;
            }
        }
        let query = query.filter(|(qs, _)| Some(*qs) != self.ac_dismissed);
        match query {
            Some((qstart_byte, q)) => {
                self.ac_start = Some(qstart_byte);
                if !self.ac_active {
                    self.ac_sel = 0;
                }
                self.ac_active = true;
                self.fill_suggestions(vault, &q, text);
                if self.ac_sel >= self.ac_items.len() {
                    self.ac_sel = 0;
                }
                let picked = if let Some(pick) = self.ac_pick.take() {
                    Some(pick)
                } else if !self.ac_items.is_empty() {
                    let c = cursor.unwrap_or(0);
                    let r = output.galley.pos_from_cursor(CCursor::new(c));
                    let pos = output.galley_pos + r.left_bottom().to_vec2() + Vec2::new(0.0, 4.0);
                    let clicked = self.suggestion_popup(ui, pos);
                    if clicked.is_some() {
                        ui.ctx().memory_mut(|m| m.request_focus(self.id));
                    }
                    clicked
                } else {
                    None
                };
                if let Some((name, exists)) = picked.and_then(|i| self.ac_items.get(i).cloned()) {
                    if self.ac_people && !exists {
                        action = Some(EditorAction::CreatePerson(name.clone()));
                    }
                    let cur_byte = markdown::char_to_byte(text, cursor.unwrap_or(0));
                    let closes = text[cur_byte..].starts_with("]]");
                    let insert = if closes { name.clone() } else { format!("{name}]]") };
                    text.replace_range(qstart_byte..cur_byte, &insert);
                    self.set_cursor_byte(ui, text, qstart_byte + name.len() + 2);
                    changed = true;
                    self.ac_active = false;
                }
            }
            None => {
                self.ac_active = false;
                self.ac_start = None;
            }
        }

        // --- [! picker safety net: if a key reached the text box instead of the picker, undo what
        // the text box did with it and do the picker's job instead.
        let armed = self.picker_before.is_some();
        if let Some((pcb, pstart, ptyped)) = self.picker_before.take() {
            let before = std::mem::take(&mut self.text_before);
            let inserted = |what: &str| text.len() == before.len() + what.len() && text.get(..pcb) == before.get(..pcb) && text.get(pcb..pcb + what.len()) == Some(what) && text.get(pcb + what.len()..) == before.get(pcb..);
            let (k_down, k_up, k_esc) = ui.input(|i| (i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowUp), i.key_pressed(Key::Escape)));
            if inserted("\t") {
                *text = before.clone();
                self.set_cursor_byte(ui, text, pcb);
                self.media_tab(ui, text, &MediaTyped { vault, home: &home, start: pstart, cursor: pcb, typed: &ptyped });
                changed = true;
                media_handled = true;
            } else if inserted("\n") || inserted("\r\n") || inserted("\r") {
                *text = before.clone();
                self.set_cursor_byte(ui, text, pcb);
                let n = self.media.items.len();
                if let Some(e) = self.media.items.get(self.media.sel.min(n.saturating_sub(1))).cloned() {
                    let (_, a) = self.media_accept(ui, text, &MediaTyped { vault, home: &home, start: pstart, cursor: pcb, typed: &ptyped }, &e);
                    if a.is_some() {
                        action = a;
                    }
                }
                changed = true;
                media_handled = true;
            } else if k_esc {
                self.media.dismissed = Some(pstart);
                self.media.active = false;
                media_handled = true;
            } else if (k_down || k_up) && *text == before {
                let n = self.media.items.len();
                if n > 0 {
                    self.media.sel = if k_down { (self.media.sel + 1) % n } else { (self.media.sel + n - 1) % n };
                }
                self.set_cursor_byte(ui, text, pcb);
                media_handled = true;
                ui.ctx().request_repaint();
            }
            // keep the text box focused while the picker is in use (unless the person clicked away)
            if self.media.active && !resp.has_focus() && !ui.input(|i| i.pointer.any_pressed()) {
                ui.ctx().memory_mut(|m| m.request_focus(self.id));
                ui.ctx().request_repaint();
            }
        }

        // --- [! file picker popup
        self.media.shown = false;
        let cur_byte = output.cursor_range.filter(|r| r.is_empty()).map(|r| markdown::char_to_byte(text, r.primary.index.0));
        let keep = (media_handled || armed) && self.media.active;
        if std::env::var_os("SHALE_DEBUG").is_some() {
            if let Some(cb) = cur_byte {
                if crate::media::media_query(text, cb).is_some() && !resp.has_focus() && !keep {
                    eprintln!("picker hidden because the text box has no focus: focused={:?} me={:?} t={:.2}", ui.ctx().memory(|m| m.focused()), self.id, ui.input(|i| i.time));
                }
            }
        }
        let mq = if (resp.has_focus() || keep) && typing { cur_byte.or(self.media.cursor).and_then(|cb| crate::media::media_query(text, cb).map(|q| (cb, q))) } else { None };
        match mq {
            Some((cb, (start, typed))) if self.media.dismissed != Some(start) => {
                let (dir, partial, _) = crate::media::split_typed(&typed, &vault.root, &home);
                let key = (dir.clone(), partial.clone());
                if self.media.key.as_ref() != Some(&key) {
                    self.media.items = crate::media::list_matches(&dir, &partial);
                    self.media.key = Some(key);
                    self.media.sel = 0;
                    self.media.error = None;
                }
                self.media.active = true;
                self.media.shown = true;
                self.media.start = Some(start);
                self.media.cursor = Some(cb);
                let shown_dir = if typed.starts_with("~") {
                    format!("~/{}", dir.strip_prefix(&home).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default())
                } else {
                    format!("vault/{}", dir.strip_prefix(&vault.root).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default())
                };
                let r = output.galley.pos_from_cursor(CCursor::new(markdown::byte_to_char(text, cb)));
                let pos = output.galley_pos + r.left_bottom().to_vec2() + Vec2::new(0.0, 4.0);
                let (clicked, close, answer) = self.picker_popup(ui, pos, &shown_dir, typed.is_empty());
                if let Some(answer) = answer {
                    match (answer, self.media.asking.clone()) {
                        (Some(how), Some(src)) => {
                            let (c, a) = self.media_finish(ui, text, &MediaTyped { vault, home: &home, start, cursor: cb, typed: &typed }, &src, how);
                            changed |= c;
                            if a.is_some() {
                                action = a;
                            }
                        }
                        _ => self.media.asking = None,
                    }
                    ui.ctx().memory_mut(|m| m.request_focus(self.id));
                } else if close {
                    self.media.dismissed = Some(start);
                    self.media.active = false;
                    self.media.shown = false;
                    ui.ctx().memory_mut(|m| m.request_focus(self.id));
                    ui.ctx().request_repaint();
                } else if let Some(e) = clicked {
                    let (c, a) = self.media_accept(ui, text, &MediaTyped { vault, home: &home, start, cursor: cb, typed: &typed }, &e);
                    changed |= c;
                    if a.is_some() {
                        action = a;
                    }
                    ui.ctx().memory_mut(|m| m.request_focus(self.id));
                }
            }
            Some(_) => self.media.active = false,
            None => {
                self.media.active = false;
                self.media.key = None;
                self.media.cursor = None;
                // forget a dismissal only once that `[!` is gone from the text, so a brief loss of
                // focus or a cursor jump doesn't bring the closed picker back
                if let Some(d) = self.media.dismissed {
                    if !text.get(d..).map(|t| t.starts_with("[!")).unwrap_or(false) {
                        self.media.dismissed = None;
                    }
                }
            }
        }
        // While a suggestion popup is open, switch the desktop input method (IBus, Fcitx, dead
        // keys) off for this window: otherwise it may keep Esc and the arrow keys for itself and
        // hand Tab / Enter over as typed text.
        if self.is_open() {
            ui.ctx().output_mut(|o| o.ime = None);
        }
        if !self.media.active {
            self.media.asking = None;
            self.media.how = None;
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) && self.media.shown {
            // an Esc that arrived after the popup was drawn this frame: close on the next frame
            ui.ctx().request_repaint();
        }
        (changed, action)
    }

    /// The headings for a query with a `#` (LINK-016): `Note#fil` or `#fil` (the text being typed,
    /// `open`, for the open note). Each item is what replaces the query (`Note#Heading`), each
    /// label the heading indented by level with its number. Headings that a link cannot hold
    /// (with `|`, `[` or `]` in their text) are left out.
    fn heading_items(&self, vault: &Vault, q: &str, open: &str) -> (Vec<(String, bool)>, Vec<String>) {
        let (target, typed) = q.split_once('#').unwrap_or((q, ""));
        let note;
        let text = if target.trim().is_empty() {
            open
        } else {
            note = vault.resolve(target, None).and_then(|rel| vault.notes.get(&rel)).map(|n| n.text.as_str());
            match note {
                Some(t) => t,
                None => return (vec![], vec![]),
            }
        };
        let numbered = markdown::headings_numbered(text, self.numbered_headings);
        let typed = typed.trim();
        let mut items = Vec::new();
        let mut labels = Vec::new();
        for (_, level, title, number) in markdown::heading_numbers(text) {
            let plain = markdown::heading_plain(&title);
            if plain.is_empty() || plain.contains(['|', '[', ']']) || (!typed.is_empty() && markdown::fuzzy_score(typed, &plain).is_none()) {
                continue;
            }
            let number = number.filter(|_| numbered).map(|n| format!("{n} ")).unwrap_or_default();
            labels.push(format!("{}{number}{plain}", "\u{2003}".repeat(level - 1)));
            items.push((format!("{target}#{plain}"), true));
        }
        (items, labels)
    }

    /// The `[[` list for query `q`: notes and drawings, headings after a `#`, or people for `[[p `.
    fn fill_suggestions(&mut self, vault: &Vault, q: &str, open: &str) {
        if q.contains('#') && !q.starts_with("p ") {
            self.ac_people = false;
            (self.ac_items, self.ac_labels) = self.heading_items(vault, q, open);
            return;
        }
        let people_q = q.strip_prefix("p ").map(|r| r.to_string());
        self.ac_people = people_q.is_some();
        let mut labels: Vec<String> = Vec::new();
        let mut list: Vec<(String, bool)> = Vec::new();
        if let Some(pq) = people_q {
            let dir = self.people_dir.trim().trim_matches('/').to_lowercase();
            let prefix = format!("{dir}/");
            let mut items: Vec<(i32, String, std::path::PathBuf)> = vault
                .files
                .iter()
                .filter(|f| f.kind == crate::vault::FileKind::Note && !dir.is_empty() && crate::vault::rel_to_string(&f.rel).to_lowercase().starts_with(&prefix))
                .filter_map(|f| markdown::fuzzy_score(&pq, &f.display_name()).map(|s| (s, f.display_name(), f.rel.clone())))
                .collect();
            items.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            for (_, name, rel) in items.into_iter().take(8) {
                let link = crate::import::people::link_to(vault, &rel);
                list.push((link.trim_start_matches("[[").trim_end_matches("]]").to_string(), true));
                labels.push(name);
            }
            let qt = pq.trim();
            if !qt.is_empty() && !labels.iter().any(|n| crate::vault::name_key(n) == crate::vault::name_key(qt)) {
                list.push((markdown::link_safe_name(qt), false));
                labels.push(format!("＋ New contact “{}” in {}", markdown::link_safe_name(qt), self.people_dir.trim().trim_matches('/')));
            }
        } else {
            let mut items: Vec<(i32, String)> = vault.files.iter().filter(|f| f.kind != crate::vault::FileKind::Other || q.contains('.')).filter_map(|f| markdown::fuzzy_score(q, &f.link_name).map(|s| (s, f.link_name.clone()))).collect();
            items.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            items.dedup_by(|a, b| a.1 == b.1);
            list = items.into_iter().take(8).map(|(_, n)| (n, true)).collect();
            let qt = q.trim();
            if !qt.is_empty() && !list.iter().any(|(n, _)| crate::vault::name_key(n) == crate::vault::name_key(qt)) {
                list.push((qt.to_string(), false));
            }
            labels = list.iter().map(|(n, e)| if *e { n.clone() } else { format!("＋ Create “{n}”") }).collect();
        }
        self.ac_items = list;
        self.ac_labels = labels;
    }

    /// Draw the `[[` list at `pos`; returns the entry clicked.
    fn suggestion_popup(&self, ui: &Ui, pos: egui::Pos2) -> Option<usize> {
        let mut clicked = None;
        egui::Area::new(self.id.with("ac")).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(260.0);
                if self.ac_people {
                    ui.label(egui::RichText::new(format!("People in “{}”", self.people_dir.trim().trim_matches('/'))).small().color(theme::palette().muted));
                }
                for (i, (_, exists)) in self.ac_items.iter().enumerate() {
                    let text = self.ac_labels.get(i).cloned().unwrap_or_default();
                    let label = if *exists { egui::RichText::new(text).color(theme::palette().text) } else { egui::RichText::new(text).color(theme::palette().link_missing) };
                    if ui.selectable_label(i == self.ac_sel, label).clicked() {
                        clicked = Some(i);
                    }
                }
                ui.label(egui::RichText::new("↑↓ or Ctrl+J/K to move · Enter or click to pick · Esc to close").small().color(theme::palette().faint));
            });
        });
        clicked
    }

    /// What the footer of the `[!` picker says about files from outside the vault (MEDIA-019): what
    /// a pick does with them now, and the key that changes it for this pick.
    fn outside_hint(&self) -> String {
        match self.media.how.or(self.outside.choice()) {
            Some(Choice::Copy) => "Files from outside the vault are copied to z_media/ · Alt+Shift+L: link them instead".into(),
            Some(Choice::Link) => "Pictures, PDFs, sound and video from outside the vault are linked where they are, other files are copied · Alt+Shift+L: copy them instead".into(),
            None => "Files from outside the vault: you are asked to copy or link them · Alt+Shift+L: link them".into(),
        }
    }

    /// Draw the `[!` picker at `pos`; returns (entry clicked, close clicked, the answer to the
    /// question about the file picked: `Some(Some(how))` or `Some(None)` for Cancel).
    fn picker_popup(&self, ui: &Ui, pos: egui::Pos2, shown_dir: &str, nothing_typed: bool) -> (Option<crate::media::Entry>, bool, Option<Option<Choice>>) {
        let mut clicked: Option<crate::media::Entry> = None;
        let mut close = false;
        let mut answer = None;
        egui::Area::new(self.id.with("media")).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(320.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("Insert file from {shown_dir}")).small().color(theme::palette().muted));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").on_hover_text("Close (Esc)").clicked() {
                            close = true;
                        }
                    });
                });
                if let Some(src) = &self.media.asking {
                    let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    ui.label(egui::RichText::new(format!("“{name}” is outside the vault.")).color(theme::palette().text));
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("Copy into the vault").strong()).on_hover_text("C or Enter").clicked() {
                            answer = Some(Some(Choice::Copy));
                        }
                        if ui.button("Link where it is").on_hover_text("L").clicked() {
                            answer = Some(Some(Choice::Link));
                        }
                        if ui.button("Cancel").on_hover_text("Esc").clicked() {
                            answer = Some(None);
                        }
                    });
                    ui.label(egui::RichText::new("C copy · L link · Esc cancel").small().color(theme::palette().faint));
                    return;
                }
                if nothing_typed {
                    ui.label(egui::RichText::new("Type / for the vault or ~/ for your home folder").small().color(theme::palette().faint));
                }
                if self.media.items.is_empty() {
                    ui.label(egui::RichText::new("Nothing matches here").color(theme::palette().faint));
                }
                let total = self.media.items.len();
                let first = self.media.sel.saturating_sub(9).min(total.saturating_sub(12));
                for (i, e) in self.media.items.iter().enumerate().skip(first).take(12) {
                    let label = if e.is_dir {
                        egui::RichText::new(format!("▸ {}/", e.name)).color(theme::palette().text)
                    } else if crate::media::is_image(Path::new(&e.name)) {
                        egui::RichText::new(format!("▣ {}", e.name)).color(theme::palette().link)
                    } else {
                        egui::RichText::new(format!("  {}", e.name)).color(theme::palette().text)
                    };
                    if ui.selectable_label(i == self.media.sel, label).clicked() {
                        clicked = Some(e.clone());
                    }
                }
                if total > 12 {
                    ui.label(egui::RichText::new(format!("{total} matches")).small().color(theme::palette().faint));
                }
                if let Some(err) = &self.media.error {
                    ui.colored_label(theme::palette().link_missing, err);
                }
                ui.label(egui::RichText::new("Tab completes the path · ↑↓ or Ctrl+J/K choose · Enter picks · Esc closes").small().color(theme::palette().faint));
                ui.label(egui::RichText::new(self.outside_hint()).small().color(theme::palette().faint));
            });
        });
        (clicked, close, answer)
    }

    /// Tab in the `[!` picker: only ever completes the path, never finishes the link.
    /// 1) extend to the longest common part / unique match (like zsh);
    /// 2) otherwise put the highlighted entry into the path (folders get a `/`).
    fn media_tab(&mut self, ui: &Ui, text: &mut String, m: &MediaTyped) -> bool {
        let MediaTyped { vault, home, start, cursor, typed } = *m;
        let n = self.media.items.len();
        let (_, _, typed_dir) = crate::media::split_typed(typed, &vault.root, home);
        let completed = crate::media::tab_complete(typed, &vault.root, home).or_else(|| {
            let e = self.media.items.get(self.media.sel.min(n.saturating_sub(1)))?;
            let full = format!("{typed_dir}{}{}", e.name, if e.is_dir { "/" } else { "" });
            (full != typed).then_some(full)
        });
        match completed {
            Some(done) => {
                text.replace_range(start + 2..cursor, &done);
                self.set_cursor_byte(ui, text, start + 2 + done.len());
                self.media.sel = 0;
                self.media.key = None;
                true
            }
            None => false,
        }
    }

    /// Pick an entry in the `[!` picker: folders open, files are brought into the vault or linked
    /// where they are (MEDIA-019): by what Alt+Shift+L or the setting says, or after asking.
    fn media_accept(&mut self, ui: &Ui, text: &mut String, m: &MediaTyped, e: &crate::media::Entry) -> (bool, Option<EditorAction>) {
        let MediaTyped { vault, home, start, cursor, typed } = *m;
        let (dir, _partial, typed_dir) = crate::media::split_typed(typed, &vault.root, home);
        if e.is_dir {
            let done = format!("{typed_dir}{}/", e.name);
            text.replace_range(start + 2..cursor, &done);
            self.set_cursor_byte(ui, text, start + 2 + done.len());
            return (true, None);
        }
        let src = dir.join(&e.name);
        match self.media.how.or(self.outside.choice()) {
            Some(how) => self.media_finish(ui, text, m, &src, how),
            None if crate::media::intake::asks(&vault.root, std::slice::from_ref(&src)) => {
                self.media.asking = Some(src);
                (false, None)
            }
            None => self.media_finish(ui, text, m, &src, Choice::Copy),
        }
    }

    /// Bring the picked file `src` into the vault as `how` says and put its link in the text.
    fn media_finish(&mut self, ui: &Ui, text: &mut String, m: &MediaTyped, src: &Path, how: Choice) -> (bool, Option<EditorAction>) {
        let MediaTyped { vault, start, cursor, .. } = *m;
        match crate::media::intake::bring(&vault.root, vault.libraries(), src, how, crate::media::local_date()) {
            Ok(taken) => {
                let link = taken.link();
                // `[[!…` written as the start marker: replace the extra `[` too
                let begin = if start > 0 && text[..start].ends_with('[') { start - 1 } else { start };
                // swallow closing brackets right after the path (`]` or `]]`)
                let rest = &text[cursor..];
                let end = cursor
                    + if rest.starts_with("]]") {
                        2
                    } else if rest.starts_with(']') {
                        1
                    } else {
                        0
                    };
                text.replace_range(begin..end, &link);
                self.set_cursor_byte(ui, text, begin + link.len());
                self.media.active = false;
                self.media.key = None;
                self.media.asking = None;
                (true, (!matches!(taken, crate::media::intake::Taken::Linked { .. })).then_some(EditorAction::FilesChanged))
            }
            Err(err) => {
                self.media.asking = None;
                self.media.error = Some(format!("Could not add the file: {err}"));
                (false, None)
            }
        }
    }
}

/// ↓ or Ctrl+J in an open list: take the key, say whether it was pressed (EDIT-001, EDIT-002).
fn down_key(i: &mut egui::InputState) -> bool {
    i.consume_key(Modifiers::NONE, Key::ArrowDown) || i.consume_key(Modifiers::CTRL, Key::J)
}

/// ↑ or Ctrl+K in an open list.
fn up_key(i: &mut egui::InputState) -> bool {
    i.consume_key(Modifiers::NONE, Key::ArrowUp) || i.consume_key(Modifiers::CTRL, Key::K)
}

/// If the cursor sits right after an unfinished `[[query`, returns (byte where the query starts, query).
pub fn link_query(text: &str, cursor_char: usize) -> Option<(usize, String)> {
    let cb = markdown::char_to_byte(text, cursor_char);
    let line_start = text[..cb].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let before = &text[line_start..cb];
    let open = before.rfind("[[")?;
    let q = &before[open + 2..];
    // `[[!…` is the file picker, not a note link
    if q.starts_with('!') || q.contains("]]") || q.contains('|') || q.contains('[') || q.chars().count() > 80 {
        return None;
    }
    Some((line_start + open + 2, q.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: NET-014
    #[test]
    fn net_014_typing_a_name_in_the_other_normal_form_finds_the_note_and_offers_no_duplicate() {
        let dir = std::env::temp_dir().join(format!("shale-test-nfc-assist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Cafe\u{301}.md"), "").unwrap();
        let vault = Vault::open(&dir).unwrap();
        let mut assist = LinkAssist::new(egui::Id::unique("nfc"));
        assist.fill_suggestions(&vault, "Caf\u{e9}", "");
        assert_eq!(assist.ac_items, [("Cafe\u{301}".to_string(), true)], "the note is listed under the name on disk, and no “Create” entry for the same name");
        assist.fill_suggestions(&vault, "Caf\u{e9}s", "");
        assert_eq!(assist.ac_items, [("Caf\u{e9}s".to_string(), false)], "another name is still offered for creation");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
