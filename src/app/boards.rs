//! The boards in the window (BOARD-006 to BOARD-014): the view in the main area, the key that opens
//! it, the lanes' searches on the worker, the peek, and the one place that rewrites a board file
//! when the view changes it. The pieces are in `src/board/`.

use super::{handle_editor_action, ShaleApp, View};
use crate::board::file::{self, Board, Chip, Key};
use crate::board::peek::{Peek, With};
use crate::board::view::{self, Act, Data, Env};
use crate::link_preview::{clamp_size, size_of};
use crate::note_props;
use eframe::egui::{self, Rect, Ui};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The name of the lane that "Add lane" makes; it is renamed right away.
const NEW_LANE: &str = "New lane";

/// The note a new board starts as: the front matter that makes it a board, a title and one lane.
fn starter(title: &str) -> String {
    format!("---\ntype: board\n---\n# {title}\n\n## To do\nquery: #todo\n")
}

impl ShaleApp {
    /// The view of the boards (BOARD-007), and the peek over it.
    pub(crate) fn boards_view(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let acts = {
            let Some(vault) = self.vault.as_ref() else { return };
            let b = &mut self.boards;
            let infos = b.listing(vault).to_vec();
            // the board that was open may be gone, or no board any more: the first one is shown
            if !b.current.as_ref().is_some_and(|c| infos.iter().any(|i| i.rel == *c)) {
                b.current = infos.first().map(|i| i.rel.clone());
            }
            let keys = self.modal.is_none() && !egui::Popup::is_any_open(&ctx) && ctx.memory(|m| m.focused().is_none());
            let mut env = Env { vault, scenes: &mut self.scenes, dark: self.cfg.dark_canvas, advanced: &mut self.cfg.search.advanced };
            match b.current.clone() {
                Some(rel) => {
                    let text = vault.notes.get(&rel).map(|n| n.text.as_str()).unwrap_or_default();
                    let board = Board::parse(text);
                    let props = file::props(text);
                    let queries: Vec<&str> = board.lanes.iter().map(|l| l.query()).collect();
                    b.runs.update(&mut self.search_worker, vault, &rel, &queries);
                    let lanes = b.shown.get(vault, &rel, text, &board, props.every_lane, &b.runs);
                    let data = Data { boards: &infos, current: Some(&rel), props: &props, lanes, selected: b.selected.as_deref(), keys };
                    view::show(ui, &mut b.view, &data, &mut env)
                }
                None => {
                    let props = file::props("");
                    let data = Data { boards: &infos, current: None, props: &props, lanes: &[], selected: None, keys: false };
                    view::show(ui, &mut b.view, &data, &mut env)
                }
            }
        };
        for act in acts {
            self.board_act(&ctx, act);
        }
        self.boards_peek(&ctx);
    }

    /// What the view asked for.
    fn board_act(&mut self, ctx: &egui::Context, act: Act) {
        match act {
            Act::Switch(rel) => {
                self.close_peek();
                self.boards.view.settings = None;
                self.boards.current = Some(rel);
                self.boards.selected = None;
                self.remember_open_files();
            }
            Act::NewBoard => self.new_board(),
            Act::EditFile => {
                if let Some(rel) = self.boards.current.clone() {
                    self.open(&rel, true);
                }
            }
            Act::AddLane => {
                self.boards.view.settings = None;
                let n = self.edit_board(|b| b.add_lane(NEW_LANE)).map_or(0, |b| b.lanes.len());
                self.boards.view.rename(n.saturating_sub(1), NEW_LANE);
            }
            Act::RemoveLane(i) => {
                self.boards.view.settings = None;
                self.edit_board(|b| b.remove_lane(i));
            }
            Act::RenameLane(i, name) => {
                self.edit_board(|b| b.rename_lane(i, &name));
            }
            Act::MoveLane(from, to) => {
                self.boards.view.settings = None;
                self.edit_board(|b| b.move_lane(from, to));
            }
            Act::SetQuery(i, text) => self.set_lane_line(i, Key::Query, &text),
            Act::SetSort(i, text) => self.set_lane_line(i, Key::Sort, &text),
            Act::SetDrop(i, text) => self.set_lane_line(i, Key::Drop, &text),
            Act::SetDone(i, done) => {
                self.edit_board(|b| {
                    if let Some(l) = b.lanes.get_mut(i) {
                        l.set_done(done);
                    }
                });
            }
            Act::SetEveryLane(on) => self.edit_board_text(|t| if on { note_props::set_property(t, "cards", "every-lane") } else { note_props::remove_property(t, "cards") }),
            Act::SetShows(shows) => self.edit_board_text(|t| set_shows(t, &shows)),
            Act::Select(rel) => self.boards.selected = Some(rel),
            Act::Peek(rel, anchor) => {
                self.boards.selected = Some(rel.clone());
                self.open_peek(rel, anchor, ctx);
            }
            Act::OpenTab(rel) => {
                self.boards.selected = Some(rel.clone());
                self.open(&rel, true);
            }
            Act::SaveSettings => self.save_config(),
        }
    }

    /// Set the `key:` line of lane `i` (the form of the lane, QUERY-028).
    fn set_lane_line(&mut self, i: usize, key: Key, value: &str) {
        self.edit_board(|b| {
            if let Some(l) = b.lanes.get_mut(i) {
                l.set(key, value);
            }
        });
    }

    /// Change the board file as `f` changes the lanes: read from the note as it is now, written back
    /// as it is, so that every line the view does not know stays (BOARD-006). Gives the board as written.
    fn edit_board(&mut self, f: impl FnOnce(&mut Board)) -> Option<Board> {
        let mut made = None;
        self.edit_board_text(|text| {
            let mut b = Board::parse(text);
            f(&mut b);
            let out = b.write();
            made = Some(b);
            out
        });
        made
    }

    /// Change the text of the board note with `f`.
    fn edit_board_text(&mut self, f: impl FnOnce(&str) -> String) {
        let Some(rel) = self.boards.current.clone() else { return };
        let Some(seen) = self.vault.as_ref().and_then(|v| v.notes.get(&rel)).map(|n| n.text.clone()) else { return };
        self.edit_note_text(&rel, &seen, |t| Some(f(t)));
    }

    /// Change the text of the note `rel` with `f`, which is given what the note says and answers
    /// with the new text, or none. `expected` is the text the change was made for (what the view
    /// drew): when the note says something else now, nothing changes and the next frame shows
    /// the new state. An open buffer is changed and saved as the editor saves it (DATA-003, an
    /// undo step each side), a note without one is edited on disk.
    pub(crate) fn edit_note_text(&mut self, rel: &Path, expected: &str, f: impl FnOnce(&str) -> Option<String>) {
        let Some(_tree) = self.hold_sync_tree() else { return };
        if let Some(doc) = self.notes.get_mut(rel) {
            if doc.unreadable.is_some() {
                self.toast(format!("“{}” could not be read, so it is not changed", crate::vault::rel_to_string(rel)));
                return;
            }
            if doc.text != expected {
                return;
            }
            let Some(new) = f(&doc.text).filter(|n| *n != doc.text) else { return };
            self.undo.of(rel).checkpoint(&doc.text);
            doc.text = new;
            doc.dirty = true;
            doc.last_edit = Instant::now();
            let text = doc.text.clone();
            if let Some(v) = self.vault.as_mut() {
                v.set_note_text(rel, &text);
            }
            self.undo.of(rel).checkpoint(&text);
            self.save_note(rel);
        } else if let Some(v) = self.vault.as_mut() {
            if let Err(e) = v.edit_note(rel, |disk| if disk == expected { f(disk) } else { None }) {
                self.toast(format!("Could not change “{}”: {e}", crate::vault::rel_to_string(rel)));
            }
        }
    }

    /// Make a board note with one starter lane in the current folder and show it (BOARD-001).
    fn new_board(&mut self) {
        let name = self.with_date_prefix("Board");
        let folder = self.boards.current.as_deref().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_else(|| self.current_folder());
        let Some(v) = self.vault.as_mut() else { return };
        let rel = v.new_note_path(&folder, &name);
        match v.create_file(&rel, &starter("Board")) {
            Ok(rel) => {
                self.close_peek();
                self.boards.current = Some(rel);
                self.boards.selected = None;
            }
            Err(e) => self.toast(format!("Could not create the board: {e}")),
        }
    }

    /// Open the peek of a card (BOARD-012): its note in a buffer, next to the card.
    fn open_peek(&mut self, rel: PathBuf, anchor: Rect, ctx: &egui::Context) {
        if self.boards.peek.as_ref().is_some_and(|p| p.rel == rel) {
            return;
        }
        self.close_peek();
        if !self.vault.as_ref().is_some_and(|v| v.file(&rel).is_some_and(|f| f.kind == crate::vault::FileKind::Note)) {
            // (a drawing has no text to edit: its card opens in a tab)
            self.open(&rel, true);
            return;
        }
        self.ensure_buffer(&rel);
        let size = clamp_size(size_of(&self.cfg), ctx.content_rect().size());
        self.boards.peek = Some(Peek::new(rel, anchor, size));
    }

    /// Close the peek (Esc, BOARD-012); what was typed is saved.
    pub(crate) fn close_peek(&mut self) {
        if let Some(peek) = self.boards.peek.take() {
            self.save_note(&peek.rel);
        }
    }

    /// Draw the peek and carry out what was done in it (BOARD-013).
    fn boards_peek(&mut self, ctx: &egui::Context) {
        let (Some(peek), Some(vault)) = (self.boards.peek.as_mut(), self.vault.as_ref()) else { return };
        let rel = peek.rel.clone();
        let Some(doc) = self.notes.get_mut(&rel) else {
            self.boards.peek = None;
            return;
        };
        let fonts = self.fonts.editor();
        peek.editor.apply_settings(&self.cfg, &self.vdata.settings.people_dir, &self.media);
        let title = vault.file(&rel).map(|f| f.title()).unwrap_or_default();
        let priority = note_props::priority(&doc.text);
        let out = peek.show(ctx, With { doc, vault, undo: &mut self.undo, fonts: &fonts, ctrl_to_follow: self.cfg.ctrl_to_follow, title: &title, priority });
        if out.changed {
            if let Some(doc) = self.notes.get_mut(&rel) {
                doc.dirty = true;
                doc.last_edit = Instant::now();
                let text = doc.text.clone();
                if let Some(v) = self.vault.as_mut() {
                    v.set_note_text(&rel, &text);
                }
            }
        }
        if let Some(p) = out.priority {
            // the front matter only: the body is the same, so `modified:` stays (VAULT-046)
            let seen = self.notes.get(&rel).map(|d| d.text.clone()).unwrap_or_default();
            self.edit_note_text(&rel, &seen, |t| Some(note_props::with_priority(t, p)));
        }
        if let Some(a) = out.action {
            handle_editor_action(&mut self.actions, a, &rel);
        }
        if let Some(size) = out.resized {
            // the size is the setting every preview uses (EDIT-037)
            self.cfg.link_preview_width = size.x.round() as u32;
            self.cfg.link_preview_height = size.y.round() as u32;
            self.save_config();
        }
        if out.open_tab {
            self.close_peek();
            self.open(&rel, true);
        } else if out.close {
            self.close_peek();
        }
    }

    /// The keys of the boards that come before the editors: Esc closes the peek (BOARD-012), also
    /// when its editor has the keyboard, except where Esc leaves the insert mode of vim-style editing.
    /// A peek of a view that is not shown any more is closed.
    pub(crate) fn boards_keys(&mut self, ui: &Ui) {
        if self.tabbar.tabs.get(self.tabbar.active).map(|t| &t.view) != Some(&View::Boards) {
            self.boards.view.settings = None;
            self.close_peek();
            return;
        }
        let Some(peek) = self.boards.peek.as_ref() else { return };
        let in_insert = peek.editor.vim_enabled && peek.editor.vim.mode != crate::vim::Mode::Normal;
        if !in_insert && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.close_peek();
        }
    }
}

/// `text` with `card-shows:` set to `shows`; all the chips are the default and need no line.
fn set_shows(text: &str, shows: &[Chip]) -> String {
    if shows == Chip::ALL {
        note_props::remove_property(text, "card-shows")
    } else {
        note_props::set_property(text, "card-shows", &shows.iter().map(|c| c.word()).collect::<Vec<_>>().join(", "))
    }
}
