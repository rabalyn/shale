//! The keyboard of the lists of drafts and outbox, the menu of a row and the window of the sent
//! mails that are only on this computer (MAIL-067…MAIL-069). The keys are actions of the registry
//! (scope Mail view); `MailView::handle_keys` calls in here.

use crate::config::Config;
use crate::keys::Action as KeyAction;
use crate::mail_compose::{LocalOut, LocalView};
use crate::mail_ui::*;
use crate::mail_view::Act;
use crate::outbox::{DraftInfo, Item, State};
use eframe::egui::{self, Pos2, Rect, Ui, Vec2};

/// What a menu of a row belongs to: the row of the message list or of the list of drafts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuTarget {
    Message(usize),
    Draft(usize),
}

/// The open menu of a row: where it is, and which entry is marked (↑ ↓ move it, Enter runs it).
#[derive(Clone, Debug)]
pub struct RowMenu {
    pub target: MenuTarget,
    pub at: Pos2,
    pub sel: usize,
}

#[derive(Clone, Copy, Debug)]
enum MenuCmd {
    Act(Act),
    DiscardDraft,
}

/// What the keyboard adds to the mail view, as one sub-state of `MailView`.
#[derive(Default)]
pub struct Kbd {
    pub menu: Option<RowMenu>,
    /// The marked entry of the window of the unsent copies.
    pub unsent_sel: usize,
}

impl MailView {
    fn drafts_here(&self) -> Vec<DraftInfo> {
        match &self.local_view {
            Some(LocalView::Drafts(a)) => self.local_drafts.iter().filter(|d| &d.account == a).cloned().collect(),
            _ => vec![],
        }
    }

    fn outbox_here(&self) -> Vec<Item> {
        match &self.local_view {
            Some(LocalView::Outbox(a)) => self.local_items.iter().filter(|i| &i.account == a).cloned().collect(),
            _ => vec![],
        }
    }

    fn local_len(&self) -> usize {
        match &self.local_view {
            Some(LocalView::Drafts(_)) => self.drafts_here().len(),
            Some(LocalView::Outbox(_)) => self.outbox_here().len(),
            None => 0,
        }
    }

    /// The cursor stays inside the list (a row may have gone).
    fn clamp_local_cursor(&mut self) {
        let n = self.local_len();
        if let Some(c) = self.cursor {
            self.cursor = if n == 0 { None } else { Some(c.min(n - 1)) };
        }
    }

    /// j / k in a list of drafts or outbox. In the outbox the mail under the cursor is the selected one.
    pub(crate) fn local_step(&mut self, delta: isize) {
        let n = self.local_len();
        if n == 0 {
            return;
        }
        let at = match self.cursor {
            Some(c) => (c as isize + delta).clamp(0, n as isize - 1) as usize,
            None if delta < 0 => n - 1,
            None => 0,
        };
        self.cursor = Some(at);
        self.local_select_cursor();
    }

    fn local_select_cursor(&mut self) {
        if let (Some(c), Some(LocalView::Outbox(_))) = (self.cursor, &self.local_view) {
            if let Some(it) = self.outbox_here().get(c) {
                self.outbox_sel = Some((it.account.clone(), it.id.clone()));
            }
        }
    }

    /// Enter in a list of drafts or outbox: open the draft, or show what happened to the mail.
    pub(crate) fn local_enter(&mut self) {
        if matches!(self.local_view, Some(LocalView::Outbox(_))) {
            self.local_select_cursor();
        } else {
            self.local_edit();
        }
    }

    /// The mail of the outbox that is selected.
    fn outbox_item(&self) -> Option<Item> {
        self.outbox_sel.as_ref().and_then(|(a, i)| self.local_items.iter().find(|x| &x.account == a && &x.id == i)).cloned()
    }

    /// `e`: open the draft; take a mail of the outbox that was not sent back into the editor.
    pub(crate) fn local_edit(&mut self) {
        let out = match &self.local_view {
            Some(LocalView::Drafts(_)) => self.cursor.and_then(|c| self.drafts_here().get(c).map(|d| (d.account.clone(), d.id.clone()))).map(|k| LocalOut { open_draft: Some(k), ..Default::default() }),
            Some(LocalView::Outbox(_)) => self.outbox_item().filter(|i| matches!(i.state, State::Failed | State::Unsure)).map(|i| LocalOut { edit: Some((i.account, i.id)), ..Default::default() }),
            None => None,
        };
        if let Some(out) = out {
            self.apply_local_out(out);
        }
    }

    /// Shift+S: send a mail that failed again, or one that may have been sent (the person looked in Sent).
    pub(crate) fn local_retry(&mut self) {
        let Some(it) = self.outbox_item() else { return };
        let key = Some((it.account.clone(), it.id.clone()));
        match it.state {
            State::Failed => self.apply_local_out(LocalOut { retry: key, ..Default::default() }),
            State::Unsure => self.apply_local_out(LocalOut { send_again: key, ..Default::default() }),
            _ => {}
        }
    }

    /// d / Delete: discard the draft, or the record of the outbox (the outbox refuses one that waits to be sent).
    pub(crate) fn local_discard(&mut self) {
        let out = match &self.local_view {
            Some(LocalView::Drafts(_)) => self.cursor.and_then(|c| self.drafts_here().get(c).map(|d| (d.account.clone(), d.id.clone()))).map(|k| LocalOut { discard_draft: Some(k), ..Default::default() }),
            Some(LocalView::Outbox(_)) => self.outbox_item().map(|i| LocalOut { discard: Some((i.account, i.id)), ..Default::default() }),
            None => None,
        };
        if let Some(out) = out {
            self.apply_local_out(out);
            self.clamp_local_cursor();
        }
    }

    /// z: take back the mail of the outbox that is selected and waiting; else the newest waiting one.
    pub(crate) fn undo_key(&mut self) {
        let selected = self.outbox_item().filter(|i| i.state == State::Queued && matches!(self.local_view, Some(LocalView::Outbox(_))));
        match selected {
            Some(i) => self.apply_local_out(LocalOut { undo: Some((i.account, i.id)), ..Default::default() }),
            None => self.undo_newest(),
        }
    }

    // ------------------------------------------------------------ the menu of a row

    /// Open the menu at the pointer (a right click on a row).
    pub(crate) fn open_menu_at(&mut self, target: MenuTarget, at: Pos2) {
        self.kbd.menu = Some(RowMenu { target, at, sel: 0 });
    }

    /// Menu / Shift+F10: the menu of the row under the cursor, next to the row.
    pub(crate) fn open_menu_by_key(&mut self) {
        let drafts = self.drafts_here().len();
        let local = self.local_view.clone();
        let (target, key) = match (local, self.cursor) {
            (Some(LocalView::Drafts(_)), Some(c)) if c < drafts => (MenuTarget::Draft(c), format!("mail:draft:{c}")),
            (None, Some(c)) if self.cursor_message().is_some() => (MenuTarget::Message(c), format!("mail:row:{c}")),
            _ => return,
        };
        let at = self.rects.iter().rev().find(|r| r.0 == key).map(|r| Pos2::new(r.1.min.x + 40.0, r.1.center().y)).unwrap_or(Pos2::new(200.0, 200.0));
        self.open_menu_at(target, at);
    }

    fn menu_entries(&self, target: MenuTarget) -> Vec<(String, String, MenuCmd)> {
        match target {
            MenuTarget::Message(i) => {
                let Some(m) = self.row(i) else { return vec![] };
                let read = if m.is_seen() { "Mark unread" } else { "Mark read" };
                let flag = if m.is_flagged() { "Unflag" } else { "Flag" };
                let del = if self.in_trash(m) { "Delete forever…" } else { "Delete" };
                [("mail:ctx-read", read, Act::Read), ("mail:ctx-flag", flag, Act::Flag), ("mail:ctx-move", "Move…", Act::Move), ("mail:ctx-delete", del, Act::Delete)]
                    .into_iter()
                    .map(|(k, l, a)| (k.to_string(), l.to_string(), MenuCmd::Act(a)))
                    .collect()
            }
            MenuTarget::Draft(i) => vec![(format!("mail:draft-discard:{i}"), "Discard draft".to_string(), MenuCmd::DiscardDraft)],
        }
    }

    /// Keys while a menu is open: ↓ / j and ↑ / k mark an entry, Enter runs it, Esc closes it.
    /// Nothing else acts (MAIL-068).
    pub(crate) fn menu_keys(&mut self, cfg: &Config, action: Option<KeyAction>, enter: bool, esc: bool) {
        let Some(menu) = self.kbd.menu.clone() else { return };
        let entries = self.menu_entries(menu.target);
        let n = entries.len();
        if esc || n == 0 {
            self.kbd.menu = None;
            return;
        }
        let step = match action {
            Some(KeyAction::MailNext) => 1,
            Some(KeyAction::MailPrev) => -1,
            _ => 0,
        };
        let sel = (menu.sel as isize + step).clamp(0, n as isize - 1) as usize;
        if let Some(m) = self.kbd.menu.as_mut() {
            m.sel = sel;
        }
        if enter {
            self.run_menu(cfg, menu.target, entries[sel].2);
        }
    }

    fn run_menu(&mut self, cfg: &Config, target: MenuTarget, cmd: MenuCmd) {
        self.kbd.menu = None;
        match (target, cmd) {
            (MenuTarget::Message(i), MenuCmd::Act(a)) => {
                // a menu on a marked row works on all marked mails, on another row only on that row
                if let Some(m) = self.row(i).cloned() {
                    self.cursor = Some(i);
                    let t = if self.marked.contains_key(&m.id) { self.marked.values().cloned().collect() } else { vec![m] };
                    self.run_act(cfg, a, t);
                }
            }
            (MenuTarget::Draft(i), MenuCmd::DiscardDraft) => {
                if let Some(d) = self.drafts_here().get(i) {
                    let out = LocalOut { discard_draft: Some((d.account.clone(), d.id.clone())), ..Default::default() };
                    self.apply_local_out(out);
                    self.clamp_local_cursor();
                }
            }
            _ => {}
        }
    }

    /// The menu itself, drawn above everything; a click outside closes it.
    pub(crate) fn row_menu_window(&mut self, ui: &mut Ui, cfg: &Config, rects: &mut Vec<(String, Rect)>) {
        let Some(menu) = self.kbd.menu.clone() else { return };
        let entries = self.menu_entries(menu.target);
        if entries.is_empty() {
            self.kbd.menu = None;
            return;
        }
        let mut chosen: Option<MenuCmd> = None;
        let area = egui::Area::new(egui::Id::unique("mail-row-menu")).order(egui::Order::Foreground).fixed_pos(menu.at).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(150.0);
                for (k, (key, label, cmd)) in entries.iter().enumerate() {
                    let b = ui.add(egui::Button::new(label.as_str()).selected(k == menu.sel).min_size(Vec2::new(ui.available_width(), 0.0)));
                    rects.push((key.clone(), b.rect));
                    if k == menu.sel {
                        rects.push(("mail:menu-sel".into(), b.rect));
                    }
                    if b.clicked() {
                        chosen = Some(*cmd);
                    }
                }
            });
        });
        if let Some(c) = chosen {
            self.run_menu(cfg, menu.target, c);
        } else if ui.input(|i| i.pointer.any_pressed()) && !ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| area.response.rect.contains(p)) {
            self.kbd.menu = None;
        }
    }

    // ------------------------------------------------------------ the window of the unsent copies

    /// While the window of the unsent copies is open it has the keyboard: j / k mark a mail, Enter uploads
    /// it again, d / Delete dismisses it, Esc or Shift+U close the window (MAIL-069). Returns whether the
    /// window took the key.
    pub(crate) fn unsent_keys(&mut self, cfg: &Config, action: Option<KeyAction>, enter: bool, esc: bool) -> bool {
        if !self.unsent_open || self.unsent.is_empty() {
            return false;
        }
        let n = self.unsent.len();
        let sel = self.kbd.unsent_sel.min(n - 1);
        match action {
            Some(KeyAction::MailNext) => self.kbd.unsent_sel = (sel + 1).min(n - 1),
            Some(KeyAction::MailPrev) => self.kbd.unsent_sel = sel.saturating_sub(1),
            Some(KeyAction::MailDelete) => {
                let c = self.unsent[sel].clone();
                self.unsent_dismiss(&[c]);
            }
            Some(KeyAction::MailUnsent) => self.unsent_open = false,
            _ => {}
        }
        if enter {
            let c = self.unsent[sel].clone();
            self.unsent_upload(cfg, &[c]);
        }
        if esc {
            self.unsent_open = false;
        }
        true
    }

    /// Shift+U with the window closed: open it, the first mail marked.
    pub(crate) fn unsent_show(&mut self) {
        if !self.unsent.is_empty() {
            self.unsent_open = true;
            self.kbd.unsent_sel = 0;
        }
    }
}
