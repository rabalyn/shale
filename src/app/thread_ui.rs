//! The thread overview in the right sidebar (NAV-070) and the keys of the thread (KEY-050): the
//! notes with the `thread-id` of the open note, oldest first, and a step to the previous or next
//! one. The data is `thread.rs`; nothing here knows what an item is.

use super::{muted, Action, RightPanel, ShaleApp};
use crate::sidebar::EQUAL;
use crate::theme;
use crate::thread::{self, Index, Row};
use crate::vault::Vault;
use eframe::egui::{self, Sense, Ui};
use std::path::PathBuf;

/// What the overview keeps between frames.
#[derive(Default)]
pub struct ThreadState {
    /// the index of the vault's thread data, for the vault and the generation it was made for
    cache: Option<(u64, u64, Index)>,
    /// the list has the keyboard (Ctrl+Shift+T, KEY-050)
    pub kbd: bool,
    /// the row the keyboard marks
    pub sel: usize,
}

impl ThreadState {
    /// The index of a vault is kept (tests).
    #[cfg(test)]
    pub(crate) fn has_index(&self) -> bool {
        self.cache.is_some()
    }

    /// The index of `vault`, made again only when the vault changed (PERF-001).
    pub(crate) fn index(&mut self, vault: &Vault) -> &Index {
        let stale = self.cache.as_ref().is_none_or(|(uid, generation, _)| *uid != vault.uid || *generation != vault.generation);
        if stale {
            self.cache = Some((vault.uid, vault.generation, Index::build(vault)));
        }
        &self.cache.as_ref().expect("just built").2
    }
}

impl ShaleApp {
    /// The thread of the open note: `None` when there is no note or it has no `thread-id`.
    fn thread_rows(&mut self) -> Option<(PathBuf, Vec<Row>)> {
        let cur = self.current_path()?;
        let v = self.vault.as_ref()?;
        let rows = self.thread.index(v).thread_of(&cur)?;
        Some((cur, rows))
    }

    /// Open the previous (`-1`) or next (`1`) note of the thread, in the order of the overview; at
    /// the ends and without a thread nothing happens (KEY-050).
    pub(crate) fn step_thread(&mut self, delta: isize) {
        if let Some((cur, rows)) = self.thread_rows() {
            if let Some(target) = thread::step(&rows, &cur, delta) {
                self.open(&target, false);
            }
        }
    }

    /// Show the Thread view in the right sidebar and give it the keyboard (KEY-050).
    pub(crate) fn show_thread_overview(&mut self, ctx: &egui::Context) {
        if !self.side.layout.show_panel(RightPanel::Thread) {
            self.side.peek = Some(RightPanel::Thread);
            self.side.peek_weight = EQUAL;
        }
        self.cfg.show_right = true;
        self.thread.sel = self.thread_rows().and_then(|(cur, rows)| rows.iter().position(|r| r.rel == cur)).unwrap_or(0);
        self.thread.kbd = true;
        if let Some(f) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(f));
        }
    }

    /// The Thread view of the right sidebar (NAV-070).
    pub(crate) fn thread_panel_body(&mut self, ui: &mut Ui) {
        let Some((cur, rows)) = self.thread_rows() else {
            self.thread.kbd = false;
            let text = if self.current_path().is_none() { "Open a note to see its thread." } else { "This note has no thread. Mails imported by Shale carry a thread-id, taken from their reply headers." };
            ui.add(egui::Label::new(muted(text)).wrap());
            return;
        };
        if rows.len() < 2 {
            self.thread.kbd = false;
            ui.add(egui::Label::new(muted("No other note in this thread yet.")).wrap());
            return;
        }
        // the keyboard: ↑ ↓ move, Enter jumps, Esc gives it back to the note; a text field with the focus ends it
        if self.thread.kbd && (self.shortcuts_blocked() || ui.ctx().memory(|m| m.focused()).is_some()) {
            self.thread.kbd = false;
        }
        self.thread.sel = self.thread.sel.min(rows.len() - 1);
        let mut open = None;
        if self.thread.kbd {
            let (up, down, enter, esc) = ui.input_mut(|i| {
                let k = |i: &mut egui::InputState, key| i.consume_key(egui::Modifiers::NONE, key);
                (k(i, egui::Key::ArrowUp), k(i, egui::Key::ArrowDown), k(i, egui::Key::Enter), k(i, egui::Key::Escape))
            });
            if up {
                self.thread.sel = self.thread.sel.saturating_sub(1);
            }
            if down {
                self.thread.sel = (self.thread.sel + 1).min(rows.len() - 1);
            }
            if enter {
                open = Some(rows[self.thread.sel].rel.clone());
                self.thread.kbd = false;
            }
            if esc {
                self.thread.kbd = false;
                self.focus_editor();
            }
        }
        let Some(v) = &self.vault else { return };
        let p = theme::palette();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (k, row) in rows.iter().enumerate() {
                let current = row.rel == cur;
                let marked = self.thread.kbd && k == self.thread.sel;
                let title = v.file(&row.rel).map(|f| f.display_name()).unwrap_or_default();
                let fill = if current {
                    p.accent.gamma_multiply(0.25)
                } else if marked {
                    p.faint_bg
                } else {
                    egui::Color32::TRANSPARENT
                };
                let frame = egui::Frame::new().fill(fill).corner_radius(4).inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let who = row.who.trim();
                    let head = if who.is_empty() { row.date_key.clone() } else { format!("{}  ·  {who}", row.date_key) };
                    ui.add(egui::Label::new(egui::RichText::new(head).size(11.5).color(p.muted)).truncate());
                    let title = egui::RichText::new(title).size(13.5).color(if current { p.text_strong } else { p.link });
                    ui.add(egui::Label::new(if current { title.strong() } else { title }).truncate());
                });
                let resp = ui.interact(frame.response.rect, ui.scope_id().with(("thread-row", k)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(crate::vault::rel_to_string(&row.rel));
                if resp.clicked() && !current {
                    open = Some(row.rel.clone());
                }
            }
        });
        if let Some(rel) = open {
            self.actions.push(Action::Open { rel, new_tab: false });
        }
    }
}
