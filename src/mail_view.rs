//! Drawing the mail view and its keys (ADR-0007 phase 1b; MAIL-050…MAIL-061).
//!
//! Three panes: accounts and folders, the message list, the reading pane. The list is drawn with
//! `show_rows`: only the rows on screen are laid out, and only their pages are read from the
//! index, so a steady frame costs no query. The reading pane shows plain text only.

use crate::app::muted;
use crate::compose::Kind;
use crate::config::Config;
use crate::imap_sync::{FolderKind, NO_PASSWORD};
use crate::mail::display_name;
use crate::mail_compose::{LocalOut, LocalView};
use crate::mail_db::{MessageSummary, STATE_FULL, STATE_HEADER, STATE_TOO_LARGE};
use crate::mail_ui::*;
use crate::theme;
use eframe::egui::{self, Align, Color32, CornerRadius, FontId, Key, Layout, Modifiers, Pos2, Rect, RichText, Sense, Stroke, Ui, UiBuilder, Vec2};

/// What a button or a menu entry of the phase 2 actions asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Read,
    Flag,
    Move,
    Delete,
}

fn pending_word(kind: &str) -> &'static str {
    match kind {
        "flags" => "saving…",
        "move" => "moving…",
        "append" => "saving to Sent…",
        _ => "deleting…",
    }
}

/// Height of a message row in the list.
pub const ROW_H: f32 = 46.0;

fn rec(rects: &mut Vec<(String, Rect)>, key: impl Into<String>, r: &egui::Response) {
    rects.push((key.into(), r.rect));
}

impl MailView {
    /// Draw the view into the free space of `ui`. `modal_open`: a dialog has the keyboard.
    pub fn show(&mut self, ui: &mut Ui, cfg: &Config, modal_open: bool) -> Vec<MailAction> {
        let mut actions = Vec::new();
        let mut rects: Vec<(String, Rect)> = Vec::new();
        self.set_context(ui.ctx());
        self.handle_keys(ui, cfg, modal_open, &mut actions);
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        let bar = Rect::from_min_size(full.min, Vec2::new(full.width(), 38.0));
        ui.scope_builder(UiBuilder::new().max_rect(bar).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.set_clip_rect(bar.intersect(ui.clip_rect()));
            self.top_bar(ui, cfg, &mut actions, &mut rects);
        });
        let mut body = Rect::from_min_max(Pos2::new(full.min.x, bar.max.y), full.max);
        if self.prompt.is_some() {
            let strip = Rect::from_min_size(body.min, Vec2::new(body.width(), 34.0));
            ui.scope_builder(UiBuilder::new().max_rect(strip).layout(Layout::left_to_right(Align::Center)), |ui| self.password_strip(ui, cfg, &mut rects));
            body.min.y = strip.max.y;
        }
        if let Some(last) = self.problems.last().cloned() {
            let strip = Rect::from_min_size(body.min, Vec2::new(body.width(), 30.0));
            ui.scope_builder(UiBuilder::new().max_rect(strip).layout(Layout::left_to_right(Align::Center)), |ui| {
                ui.set_clip_rect(strip.intersect(ui.clip_rect()));
                ui.add_space(10.0);
                ui.label(RichText::new(format!("Not done: {last}")).size(12.5).color(theme::LINK_MISSING));
                let r = ui.small_button("Dismiss");
                rects.push(("mail:problem-dismiss".into(), r.rect));
                if r.clicked() {
                    self.problems.clear();
                }
            });
            body.min.y = strip.max.y;
        }
        // a mail that waits in the outbox can still be taken back (MAIL-134)
        if let Some((_, _, text)) = crate::mail_compose::queued_text(&self.local_items, crate::cal_time::now()) {
            let strip = Rect::from_min_size(body.min, Vec2::new(body.width(), 30.0));
            let mut undo = false;
            ui.scope_builder(UiBuilder::new().max_rect(strip).layout(Layout::left_to_right(Align::Center)), |ui| {
                ui.set_clip_rect(strip.intersect(ui.clip_rect()));
                ui.add_space(10.0);
                ui.label(RichText::new(text).size(12.5).color(theme::TEXT));
                let r = ui.small_button("Undo send");
                rects.push(("mail:undo-send".into(), r.rect));
                if r.clicked() {
                    undo = true;
                }
            });
            body.min.y = strip.max.y;
            if undo {
                self.undo_newest();
            }
        }
        self.unsent_strip(ui, &mut body, &mut rects);
        match &self.store {
            StoreState::Failed(e) => {
                let e = e.clone();
                ui.scope_builder(UiBuilder::new().max_rect(body.shrink(20.0)), |ui| {
                    ui.label(RichText::new("The local mail copy could not be opened.").color(theme::LINK_MISSING));
                    ui.label(muted(e));
                });
            }
            StoreState::Closed | StoreState::Opening => {
                ui.scope_builder(UiBuilder::new().max_rect(body.shrink(20.0)), |ui| {
                    ui.label(muted("Opening the local mail copy…"));
                });
            }
            StoreState::Ready(_) if cfg.mail_accounts.is_empty() => {
                ui.scope_builder(UiBuilder::new().max_rect(body.shrink(30.0)), |ui| {
                    ui.add_space(40.0);
                    ui.label(RichText::new("No mail account yet").size(20.0).color(theme::TEXT));
                    ui.label(muted("Shale reads your mail from your mail server and keeps a local copy. Syncing never changes anything on the server; only what you do yourself does (mark read, flag, move, delete)."));
                    ui.add_space(8.0);
                    let r = ui.button("Add an account…");
                    rec(&mut rects, "mail:add-account", &r);
                    if r.clicked() {
                        actions.push(MailAction::OpenAccounts);
                    }
                });
            }
            StoreState::Ready(_) if self.compose.is_some() => {
                let out = self.compose_view(ui, cfg, body, &mut rects);
                self.apply_compose_out(cfg, out);
            }
            StoreState::Ready(_) => {
                self.panes(ui, cfg, body, &mut actions, &mut rects);
                self.mover_window(ui, cfg, &mut rects);
                self.forever_window(ui, cfg, &mut rects);
                self.unsent_window(ui, cfg, &mut rects);
            }
        }
        self.rects = rects;
        actions
    }

    // ------------------------------------------------------------ keys

    fn handle_keys(&mut self, ui: &mut Ui, cfg: &Config, modal_open: bool, actions: &mut Vec<MailAction>) {
        let ctx = ui.ctx().clone();
        // while a mail is being written the compose window has the keyboard (MAIL-150)
        if self.compose.is_some() {
            return;
        }
        let search_id = egui::Id::unique("mail-search");
        let in_search = ctx.memory(|m| m.has_focus(search_id));
        if in_search && !modal_open {
            // Esc leaves the field; ↓ goes into the results (MAIL-061)
            let (esc, down) = ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::Escape), i.consume_key(Modifiers::NONE, Key::ArrowDown)));
            if esc || down {
                ctx.memory_mut(|m| m.surrender_focus(search_id));
            }
            if down && self.list_len() > 0 {
                self.cursor = Some(self.cursor.unwrap_or(0));
                self.scroll_to_cursor = true;
            }
            return;
        }
        // another field (or a dialog) has the keyboard: j and k type, they do not browse
        if modal_open || self.mover.is_some() || self.forever.is_some() || ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let (slash, next, prev, next_folder, prev_folder, enter, space, esc, import) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Slash),
                i.consume_key(Modifiers::NONE, Key::J) | i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::K) | i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::COMMAND, Key::J),
                i.consume_key(Modifiers::COMMAND, Key::K),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Space),
                i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_key(Modifiers::NONE, Key::I),
            )
        });
        // phase 3 keys: `c` compose, `r` reply, Shift+R reply all, Shift+F forward (`f` stays the flag key)
        let (compose, reply, reply_all, forward) = ui.input_mut(|i| {
            let plain = |i: &mut egui::InputState, k: Key| !i.modifiers.any() && i.consume_key(Modifiers::NONE, k);
            // the Shift keys first: a plain pattern would take them too (egui ignores an extra Shift)
            let ra = i.consume_key(Modifiers::SHIFT, Key::R);
            let fw = i.consume_key(Modifiers::SHIFT, Key::F);
            let c = plain(i, Key::C);
            let r = plain(i, Key::R);
            (c, r, ra, fw)
        });
        if compose || reply || reply_all || forward {
            // the letter that came with the key must not be typed into the window that opens
            ui.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Text(_))));
        }
        if compose {
            self.compose_new(cfg);
        }
        if reply {
            self.start_reply(Kind::Reply);
        }
        if reply_all {
            self.start_reply(Kind::ReplyAll);
        }
        if forward {
            self.start_reply(Kind::Forward);
        }
        // phase 2 keys: no modifier at all, so Ctrl+D and Shift+S stay free (MAIL-110)
        let (read, flag, del, mv) = ui.input_mut(|i| {
            let mut plain = |k: Key| !i.modifiers.any() && i.consume_key(Modifiers::NONE, k);
            (plain(Key::U), plain(Key::S) | plain(Key::F), plain(Key::D) | plain(Key::Delete), plain(Key::M))
        });
        if read {
            self.toggle_read(cfg);
        }
        if flag {
            self.toggle_flag(cfg);
        }
        if del {
            self.delete_selected(cfg);
        }
        if mv {
            // the "m" that came with the key must not be typed into the picker's field
            ui.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Text(_))));
            self.open_mover();
        }
        if slash {
            // the "/" that came with the key must not be typed into the field
            ui.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Text(t) if t == "/")));
            self.focus_search = true;
        }
        if next {
            self.move_cursor(1);
        }
        if prev {
            self.move_cursor(-1);
        }
        if next_folder {
            self.step_folder(cfg, 1);
        }
        if prev_folder {
            self.step_folder(cfg, -1);
        }
        if enter {
            if let Some(m) = self.cursor_message() {
                self.open_message(m);
            }
        }
        if space {
            if let Some(m) = self.cursor_message() {
                self.toggle_mark(&m);
            }
        }
        if import {
            let t = self.import_targets();
            if !t.is_empty() {
                actions.push(MailAction::Import(t));
            }
        }
        if esc {
            if self.open.is_some() {
                self.close_message();
            } else if self.results.is_some() || !self.query.is_empty() {
                self.clear_search();
            } else {
                self.marked.clear();
            }
        }
    }

    /// What "Import to vault" takes: the marked messages, else the open one, else the one under
    /// the cursor (MAIL-070).
    pub fn import_targets(&mut self) -> Vec<MessageSummary> {
        if !self.marked.is_empty() {
            return self.marked.values().cloned().collect();
        }
        if let Some(o) = &self.open {
            return vec![o.msg.clone()];
        }
        self.cursor_message().into_iter().collect()
    }

    // ------------------------------------------------------------ top bar

    fn top_bar(&mut self, ui: &mut Ui, cfg: &Config, actions: &mut Vec<MailAction>, rects: &mut Vec<(String, Rect)>) {
        ui.add_space(8.0);
        let ready = self.ready_store().is_some() && !cfg.mail_accounts.is_empty();
        if self.any_sync_running() {
            let r = ui.button("Cancel sync");
            rec(rects, "mail:cancel", &r);
            if r.clicked() {
                self.cancel_syncs();
            }
        } else {
            let r = ui.add_enabled(ready, egui::Button::new("Sync"));
            rec(rects, "mail:sync", &r);
            if r.on_hover_text("Fetch new mail from the server (read-only)").clicked() {
                self.start_sync_all(cfg);
            }
        }
        let r = ui.add_enabled(ready, egui::Button::new("Compose"));
        rec(rects, "mail:compose", &r);
        if r.on_hover_text("Write a new mail (c)").clicked() {
            self.compose_new(cfg);
        }
        let r = ui.button("Accounts…");
        rec(rects, "mail:accounts", &r);
        if r.clicked() {
            actions.push(MailAction::OpenAccounts);
        }
        ui.add_space(10.0);
        let id = egui::Id::unique("mail-search");
        if std::mem::take(&mut self.focus_search) {
            let mut st = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
            let n = self.query.chars().count();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
            st.store(ui.ctx(), id);
            ui.ctx().memory_mut(|m| m.request_focus(id));
        }
        let r = ui.add(egui::TextEdit::singleline(&mut self.query).id(id).hint_text("Search mail  (from:  to:  subject:  \"phrase\")  ·  /").desired_width(300.0));
        rec(rects, "mail:search", &r);
        if r.changed() {
            self.query_edited();
        }
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            self.search_now();
            self.to_results = true;
        }
        let before = self.search_all;
        let a = ui.selectable_label(self.search_all, "All folders");
        rec(rects, "mail:scope-all", &a);
        if a.clicked() {
            self.search_all = true;
        }
        let b = ui.selectable_label(!self.search_all, "This folder");
        rec(rects, "mail:scope-folder", &b);
        if b.clicked() {
            self.search_all = false;
        }
        if before != self.search_all && !self.query.trim().is_empty() {
            self.search_now();
        }
        ui.add_space(10.0);
        if self.searching || self.search_edited_pending() {
            ui.label(muted("Searching…").size(12.0));
        } else if let Some(e) = &self.search_error {
            ui.label(RichText::new(format!("Search failed: {e}")).size(12.0).color(theme::LINK_MISSING));
        }
        // the state of the syncs
        let mut errs: Vec<String> = vec![];
        let mut added = 0;
        for r in self.syncs.values().filter(|r| !r.running()) {
            errs.extend(r.errors.iter().cloned());
            if let Some(s) = &r.finished {
                added += s.messages_added;
                for e in &s.errors {
                    if !errs.contains(e) {
                        errs.push(e.clone());
                    }
                }
            }
        }
        if let Some(t) = self.pending_text() {
            let why = self.offline.values().next().cloned();
            let l = ui.label(muted(t).size(12.0));
            if let Some(w) = why {
                l.on_hover_text(format!("The server could not be reached: {w}"));
            }
        }
        if let Some(t) = self.status(cfg) {
            ui.label(muted(t).size(12.0));
        } else if !errs.is_empty() {
            let first = errs.first().cloned().unwrap_or_default();
            let short = if first.contains(NO_PASSWORD) {
                "No password for the account".to_string()
            } else if crate::oauth::needs_sign_in(&first) {
                "sign-in expired: open the account settings and sign in again".to_string()
            } else {
                first.chars().take(90).collect()
            };
            ui.label(RichText::new(format!("Sync problem: {short}")).size(12.0).color(theme::LINK_MISSING)).on_hover_text(errs.join("\n"));
        } else if self.syncs.values().any(|r| r.finished.is_some()) {
            ui.label(muted(format!("Synced · {added} new")).size(12.0));
        }
    }

    fn password_strip(&mut self, ui: &mut Ui, cfg: &Config, rects: &mut Vec<(String, Rect)>) {
        let Some(name) = self.prompt.as_ref().map(|p| cfg.mail_accounts.iter().find(|a| a.id == p.account).map(|a| if a.name.is_empty() { a.host.clone() } else { a.name.clone() }).unwrap_or_else(|| p.account.clone())) else { return };
        ui.add_space(10.0);
        ui.label(format!("Password for {name} (kept in memory until you quit):"));
        let mut go = false;
        if let Some(p) = self.prompt.as_mut() {
            let r = crate::secrets::password_edit(ui, egui::TextEdit::singleline(&mut p.text).desired_width(200.0));
            rec(rects, "mail:password", &r);
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                go = true;
            }
        }
        let ok = ui.button("Use it");
        rec(rects, "mail:password-ok", &ok);
        if ok.clicked() || go {
            self.give_password(cfg);
        }
        if ui.button("Not now").clicked() {
            self.prompt = None;
        }
    }

    fn search_edited_pending(&self) -> bool {
        !self.query.trim().is_empty() && self.results.is_none() && self.search_error.is_none() && !self.searching
    }

    // ------------------------------------------------------------ the panes

    fn panes(&mut self, ui: &mut Ui, cfg: &Config, body: Rect, actions: &mut Vec<MailAction>, rects: &mut Vec<(String, Rect)>) {
        let w = body.width();
        let left_w = (w * 0.19).clamp(170.0, 260.0);
        let mid_w = (w * 0.36).clamp(300.0, 540.0).min(w - left_w - 200.0).max(220.0);
        let left = Rect::from_min_size(body.min, Vec2::new(left_w, body.height()));
        let mid = Rect::from_min_size(Pos2::new(left.max.x, body.min.y), Vec2::new(mid_w, body.height()));
        let right = Rect::from_min_max(Pos2::new(mid.max.x, body.min.y), body.max);
        ui.painter().rect_filled(left, 0, theme::BG_SECONDARY);
        ui.painter().line_segment([Pos2::new(left.max.x, left.min.y), Pos2::new(left.max.x, left.max.y)], Stroke::new(1.0, theme::BORDER));
        ui.painter().line_segment([Pos2::new(mid.max.x, mid.min.y), Pos2::new(mid.max.x, mid.max.y)], Stroke::new(1.0, theme::BORDER));
        ui.painter().line_segment([body.left_top(), body.right_top()], Stroke::new(1.0, theme::BORDER));
        ui.scope_builder(UiBuilder::new().max_rect(left.shrink2(Vec2::new(8.0, 6.0))), |ui| {
            ui.set_clip_rect(left.intersect(ui.clip_rect()));
            self.folder_pane(ui, cfg, rects);
        });
        ui.scope_builder(UiBuilder::new().max_rect(mid.shrink2(Vec2::new(0.0, 4.0))), |ui| {
            ui.set_clip_rect(mid.intersect(ui.clip_rect()));
            self.list_pane(ui, cfg, actions, rects);
        });
        ui.scope_builder(UiBuilder::new().max_rect(right.shrink2(Vec2::new(14.0, 8.0))), |ui| {
            ui.set_clip_rect(right.intersect(ui.clip_rect()));
            self.reading_pane(ui, cfg, actions, rects);
        });
    }

    fn folder_pane(&mut self, ui: &mut Ui, cfg: &Config, rects: &mut Vec<(String, Rect)>) {
        let mut chosen: Option<i64> = None;
        let mut chosen_local: Option<LocalView> = None;
        egui::ScrollArea::vertical().id_salt("mail-folders").auto_shrink([false, false]).show(ui, |ui| {
            for a in &cfg.mail_accounts {
                let title = if a.name.is_empty() { a.host.clone() } else { a.name.clone() };
                let folders = self.account_folders(&a.id);
                egui::CollapsingHeader::new(RichText::new(title).color(theme::TEXT_STRONG)).id_salt(("mail-account", a.id.clone())).default_open(true).show(ui, |ui| {
                    if folders.is_empty() {
                        ui.label(muted("No folders yet. Press Sync, or “Test connection” in the account settings.").size(12.0));
                    }
                    for f in &folders {
                        let (total, unread) = self.counts.get(&f.id).copied().unwrap_or((0, 0));
                        let label = self.tree_entry(f).1;
                        let text = if unread > 0 { RichText::new(format!("{label}  ({unread})")).strong().color(theme::TEXT_STRONG) } else { RichText::new(label).color(theme::TEXT) };
                        let (depth, _) = self.tree_entry(f);
                        ui.horizontal(|ui| {
                            ui.add_space(depth as f32 * 12.0);
                            let r = ui.selectable_label(self.local_view.is_none() && self.folder == Some(f.id), text);
                            rec(rects, format!("mail:folder:{}", f.id), &r);
                            let kind = crate::imap_sync::kind_of(f);
                            let tip = match kind {
                                FolderKind::Trash | FolderKind::Junk => "Never synced automatically: the list is loaded when you open it, and a message when you open that.",
                                FolderKind::All if !f.sync_enabled => "Never synced automatically (it would copy every message again): loaded when you open it.",
                                _ if !f.sync_enabled => "Not synced (see the account settings).",
                                _ => "",
                            };
                            let r = if tip.is_empty() { r } else { r.on_hover_text(tip) };
                            let _ = total;
                            if r.clicked() {
                                chosen = Some(f.id);
                            }
                        });
                    }
                    // what is being written and what waits to be sent (MAIL-151)
                    let drafts = self.local_drafts.iter().filter(|d| d.account == a.id).count();
                    let out = self.outbox_count(&a.id);
                    let attention = crate::mail_compose::attention(&self.local_items, &a.id);
                    for (key, label, view, n, warn) in [("drafts", "Drafts", LocalView::Drafts(a.id.clone()), drafts, false), ("outbox", "Outbox", LocalView::Outbox(a.id.clone()), out, attention)] {
                        let text = if n > 0 { format!("{label}  ({n})") } else { label.to_string() };
                        let rt = if warn {
                            RichText::new(text).color(theme::LINK_MISSING).strong()
                        } else if n > 0 {
                            RichText::new(text).strong().color(theme::TEXT_STRONG)
                        } else {
                            RichText::new(text).color(theme::MUTED)
                        };
                        let r = ui.selectable_label(self.local_view.as_ref() == Some(&view), rt);
                        rec(rects, format!("mail:{key}:{}", a.id), &r);
                        if r.clicked() {
                            chosen_local = Some(view);
                        }
                    }
                });
            }
        });
        if let Some(id) = chosen {
            self.select_folder(cfg, id);
        }
        if let Some(v) = chosen_local {
            self.select_local(v);
        }
    }

    fn list_pane(&mut self, ui: &mut Ui, cfg: &Config, actions: &mut Vec<MailAction>, rects: &mut Vec<(String, Rect)>) {
        if self.local_view.is_some() {
            let mut out = LocalOut::default();
            self.local_list(ui, crate::cal_time::now(), rects, &mut out);
            self.apply_local_out(out);
            return;
        }
        let title = match (&self.results, self.folder.and_then(|f| self.folder_row(f))) {
            (Some(r), _) => format!("{} result{}{}", r.len(), if r.len() == 1 { "" } else { "s" }, if r.len() >= SEARCH_LIMIT { " (the newest are shown)" } else { "" }),
            (None, Some(f)) => format!("{}  ·  {} message{}", folder_label(f), self.total, if self.total == 1 { "" } else { "s" }),
            (None, None) => "Choose a folder".to_string(),
        };
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.label(RichText::new(title).size(13.0).color(theme::MUTED));
        });
        // what is going on with a folder that is not synced
        if let (None, Some(id)) = (&self.results, self.folder) {
            let row = self.folder_row(id).cloned();
            match self.listings.get(&id) {
                Some(ListingState::Loading) => {
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        ui.label(muted("Loading the list from the server…").size(12.0));
                    });
                }
                Some(ListingState::Failed(e)) => {
                    let e = e.clone();
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(10.0);
                        ui.label(RichText::new(format!("Could not load the list: {e}")).size(12.0).color(theme::LINK_MISSING));
                        let r = ui.small_button("Try again");
                        rec(rects, "mail:retry-list", &r);
                        if r.clicked() {
                            self.list_on_demand(cfg, id, true);
                        }
                    });
                }
                Some(ListingState::Done(n)) => {
                    let n = *n;
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        ui.label(muted(format!("Not synced automatically: the list comes from the server when you open the folder ({n} new).")).size(12.0));
                        let r = ui.small_button("Refresh");
                        rec(rects, "mail:refresh-list", &r);
                        if r.clicked() {
                            self.list_on_demand(cfg, id, true);
                        }
                    });
                }
                None => {
                    if row.as_ref().is_some_and(|f| !f.sync_enabled && crate::imap_sync::kind_of(f) == FolderKind::Normal) {
                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            ui.label(muted("This folder is not synced; tick it in the account settings to fetch it.").size(12.0));
                        });
                    }
                }
            }
        }
        if !self.marked.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                let n = self.marked.len();
                let r = ui.button(format!("Import {n} to vault"));
                rec(rects, "mail:import-marked", &r);
                if r.clicked() {
                    actions.push(MailAction::Import(self.marked.values().cloned().collect()));
                }
                if ui.small_button("Clear marks").clicked() {
                    self.marked.clear();
                }
            });
            let marked: Vec<MessageSummary> = self.marked.values().cloned().collect();
            let mut act = None;
            ui.horizontal_wrapped(|ui| {
                ui.add_space(10.0);
                let read = if marked.iter().all(|m| m.is_seen()) { "Mark unread" } else { "Mark read" };
                let flag = if marked.iter().all(|m| m.is_flagged()) { "Unflag" } else { "Flag" };
                let del = if marked.iter().all(|m| self.in_trash(m)) { "Delete forever…" } else { "Delete" };
                for (key, label, a) in [("mail:marked-read", read, Act::Read), ("mail:marked-flag", flag, Act::Flag), ("mail:marked-move", "Move…", Act::Move), ("mail:marked-delete", del, Act::Delete)] {
                    let r = ui.small_button(label);
                    rec(rects, key, &r);
                    if r.clicked() {
                        act = Some(a);
                    }
                }
            });
            if let Some(a) = act {
                self.run_act(cfg, a, marked);
            }
        }
        ui.add_space(2.0);
        if self.list_len() == 0 {
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                let t = if self.results.is_some() {
                    "Nothing found."
                } else if self.folder.is_some() {
                    "No messages here."
                } else {
                    ""
                };
                ui.label(muted(t));
            });
            return;
        }
        let n = self.list_len();
        // keep the cursor row on screen when a key moved it
        let mut area = egui::ScrollArea::vertical().id_salt("mail-list").auto_shrink([false, false]);
        if std::mem::take(&mut self.scroll_to_cursor) {
            if let Some(c) = self.cursor {
                let vis = self.visible.clone();
                if vis.is_empty() || c <= vis.start {
                    area = area.vertical_scroll_offset(c as f32 * ROW_H);
                } else if c + 1 >= vis.end {
                    area = area.vertical_scroll_offset(((c + 1) as f32 * ROW_H - self.list_h).max(0.0));
                }
            }
        }
        let mut clicked: Option<(usize, bool)> = None;
        let mut ctx_act: Option<(usize, Act)> = None;
        let now = crate::cal_time::now();
        ui.spacing_mut().item_spacing.y = 0.0;
        let out = area.show_rows(ui, ROW_H, n, |ui, range| {
            self.ensure_rows(range.clone());
            for i in range.clone() {
                let Some(m) = self.row(i).cloned() else {
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
                    continue;
                };
                let r = self.paint_row(ui, i, &m, now, rects);
                rec(rects, format!("mail:row:{i}"), &r);
                if r.clicked() {
                    clicked = Some((i, ui.input(|i| i.modifiers.command)));
                }
                r.context_menu(|ui| {
                    let read = if m.is_seen() { "Mark unread" } else { "Mark read" };
                    let flag = if m.is_flagged() { "Unflag" } else { "Flag" };
                    let del = if self.in_trash(&m) { "Delete forever…" } else { "Delete" };
                    for (key, label, a) in [("mail:ctx-read", read, Act::Read), ("mail:ctx-flag", flag, Act::Flag), ("mail:ctx-move", "Move…", Act::Move), ("mail:ctx-delete", del, Act::Delete)] {
                        let b = ui.button(label);
                        rec(rects, key, &b);
                        if b.clicked() {
                            ctx_act = Some((i, a));
                            ui.close();
                        }
                    }
                });
            }
            range
        });
        self.visible = out.inner;
        self.list_h = out.inner_rect.height();
        if let Some((i, a)) = ctx_act {
            // a menu on a marked row works on all marked mails, on another row only on that row
            if let Some(m) = self.row(i).cloned() {
                self.cursor = Some(i);
                let t = if self.marked.contains_key(&m.id) { self.marked.values().cloned().collect() } else { vec![m] };
                self.run_act(cfg, a, t);
            }
        }
        if let Some((i, toggle)) = clicked {
            self.cursor = Some(i);
            if let Some(m) = self.row(i).cloned() {
                if toggle {
                    self.toggle_mark(&m);
                } else {
                    self.open_message(m);
                }
            }
        }
    }

    /// What a button, key or menu entry asks for, on these messages.
    fn run_act(&mut self, cfg: &Config, a: Act, t: Vec<MessageSummary>) {
        match a {
            Act::Read => self.toggle_read_on(cfg, t),
            Act::Flag => self.toggle_flag_on(cfg, t),
            Act::Move => self.open_mover_on(t),
            Act::Delete => self.delete_on(cfg, t),
        }
    }

    fn paint_row(&self, ui: &mut Ui, i: usize, m: &MessageSummary, now: i64, rects: &mut Vec<(String, Rect)>) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
        let p = ui.painter_at(rect);
        let cursor = self.cursor == Some(i);
        let opened = self.open.as_ref().is_some_and(|o| o.msg.id == m.id);
        if cursor {
            p.rect_filled(rect, 0, theme::ACCENT.gamma_multiply(0.25));
        } else if opened {
            p.rect_filled(rect, 0, theme::BG_ACTIVE);
        } else if resp.hovered() {
            p.rect_filled(rect, 0, theme::BG_HOVER);
        }
        if self.marked.contains_key(&m.id) {
            p.rect_filled(Rect::from_min_size(rect.min, Vec2::new(4.0, rect.height())), 0, theme::ACCENT);
        }
        p.line_segment([Pos2::new(rect.min.x, rect.max.y), Pos2::new(rect.max.x, rect.max.y)], Stroke::new(1.0, theme::BORDER));
        let leaving = m.pending.as_deref().is_some_and(|k| k != "flags" && k != "append");
        let dim = if m.gone_on_server || leaving { 0.45 } else { 1.0 };
        let unread = !m.is_seen() && m.state != STATE_TOO_LARGE;
        let strong = |c: Color32| c.gamma_multiply(dim);
        let from = if m.from.trim().is_empty() { "(unknown sender)".to_string() } else { display_name(&m.from) };
        let date = list_date(m.date, now);
        let date_g = p.layout_no_wrap(date, FontId::proportional(12.0), strong(theme::MUTED));
        let x0 = rect.min.x + 16.0;
        let date_x = rect.max.x - 10.0 - date_g.size().x;
        if unread {
            p.circle_filled(Pos2::new(rect.min.x + 8.0, rect.min.y + 14.0), 3.0, theme::ACCENT);
            rects.push((format!("mail:unread:{i}"), Rect::from_center_size(Pos2::new(rect.min.x + 8.0, rect.min.y + 14.0), Vec2::splat(8.0))));
        }
        if m.is_flagged() {
            // a small flag: a pole and a pennant
            let (x, y) = (rect.min.x + 5.0, rect.min.y + 28.0);
            let c = strong(theme::LINK_MISSING);
            p.line_segment([Pos2::new(x, y), Pos2::new(x, y + 12.0)], Stroke::new(1.4, c));
            p.add(egui::Shape::convex_polygon(vec![Pos2::new(x, y), Pos2::new(x + 8.0, y + 3.0), Pos2::new(x, y + 6.0)], c, Stroke::NONE));
            rects.push((format!("mail:flag-mark:{i}"), Rect::from_min_size(Pos2::new(x - 2.0, y), Vec2::new(11.0, 12.0))));
        }
        p.galley(Pos2::new(date_x, rect.min.y + 7.0), date_g, Color32::WHITE);
        let from_g = p.layout_no_wrap(from, FontId::proportional(14.0), strong(if unread { theme::TEXT_STRONG } else { theme::TEXT }));
        let from_clip = p.with_clip_rect(Rect::from_min_max(Pos2::new(x0, rect.min.y), Pos2::new(date_x - 8.0, rect.max.y)));
        from_clip.galley(Pos2::new(x0, rect.min.y + 6.0), from_g.clone(), Color32::WHITE);
        if unread {
            // bold: the same text once more, a little to the right (the fonts have no bold face)
            from_clip.galley(Pos2::new(x0 + 0.7, rect.min.y + 6.0), from_g, Color32::WHITE);
        }
        let mut subject = if m.subject.trim().is_empty() { "(no subject)".to_string() } else { m.subject.clone() };
        if self.results.is_some() && self.search_all {
            subject = format!("[{}] {subject}", crate::mail_ui::folder_label_of(&m.folder));
        }
        let mut right_edge = rect.max.x - 10.0;
        if m.has_attachments {
            paint_clip(&p, Pos2::new(right_edge - 6.0, rect.min.y + 33.0), strong(theme::MUTED));
            right_edge -= 18.0;
        }
        if m.state == STATE_HEADER || m.state == STATE_TOO_LARGE {
            let g = p.layout_no_wrap("not downloaded".into(), FontId::proportional(11.0), strong(theme::FAINT));
            right_edge -= g.size().x;
            p.galley(Pos2::new(right_edge, rect.min.y + 26.0), g, Color32::WHITE);
            right_edge -= 8.0;
        }
        if let Some(kind) = &m.pending {
            let g = p.layout_no_wrap(pending_word(kind).into(), FontId::proportional(11.0), strong(theme::ACCENT));
            right_edge -= g.size().x;
            let at = Pos2::new(right_edge, rect.min.y + 26.0);
            rects.push((format!("mail:pending-mark:{i}"), Rect::from_min_size(at, g.size())));
            p.galley(at, g, Color32::WHITE);
            right_edge -= 8.0;
        }
        let subj_g = p.layout_no_wrap(subject, FontId::proportional(13.0), strong(if unread { theme::TEXT } else { theme::MUTED }));
        let subj_clip = p.with_clip_rect(Rect::from_min_max(Pos2::new(x0, rect.min.y), Pos2::new(right_edge, rect.max.y)));
        subj_clip.galley(Pos2::new(x0, rect.min.y + 25.0), subj_g.clone(), Color32::WHITE);
        if unread {
            subj_clip.galley(Pos2::new(x0 + 0.6, rect.min.y + 25.0), subj_g, Color32::WHITE);
        }
        resp
    }

    fn reading_pane(&mut self, ui: &mut Ui, cfg: &Config, actions: &mut Vec<MailAction>, rects: &mut Vec<(String, Rect)>) {
        match &self.local_view {
            Some(LocalView::Outbox(_)) => {
                let mut out = LocalOut::default();
                self.outbox_detail(ui, crate::cal_time::now(), rects, &mut out);
                self.apply_local_out(out);
                return;
            }
            Some(LocalView::Drafts(_)) => {
                ui.add_space(30.0);
                ui.label(muted("Open a draft to go on writing it. Drafts are saved by themselves while you write."));
                return;
            }
            None => {}
        }
        let Some(open) = self.open.as_ref() else {
            ui.add_space(30.0);
            ui.label(muted("Select a message to read it (Enter opens the one under the cursor)."));
            return;
        };
        let msg = open.msg.clone();
        let mut load = false;
        let mut import = false;
        let mut close = false;
        let mut act: Option<Act> = None;
        let targets: Vec<MessageSummary> = if self.marked.is_empty() { vec![msg.clone()] } else { self.marked.values().cloned().collect() };
        let all_read = targets.iter().all(|m| m.is_seen());
        let all_flagged = targets.iter().all(|m| m.is_flagged());
        let all_trash = targets.iter().all(|m| self.in_trash(m));
        let mut open_att: Option<usize> = None;
        let mut save_att: Option<usize> = None;
        let mut answer: Option<Kind> = None;
        let mut strip = StripOut::default();
        egui::ScrollArea::vertical().id_salt("mail-read").auto_shrink([false, false]).show(ui, |ui| {
            ui.label(RichText::new(if msg.subject.trim().is_empty() { "(no subject)" } else { msg.subject.as_str() }).size(19.0).color(theme::TEXT_STRONG));
            ui.add_space(4.0);
            let line = |ui: &mut Ui, k: &str, v: &str| {
                if !v.trim().is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(muted(format!("{k}:")));
                        ui.add(egui::Label::new(RichText::new(v).color(theme::TEXT)).selectable(true));
                    });
                }
            };
            let (from, to, cc, date) = match &self.open.as_ref().map(|o| &o.body) {
                Some(Body::Ready(l)) if !l.mail.from.is_empty() || !l.mail.to.is_empty() => (l.mail.from.clone(), l.mail.to.clone(), l.mail.cc.clone(), l.mail.date.map(full_date).unwrap_or_else(|| l.mail.date_raw.clone())),
                _ => (msg.from.clone(), msg.to.clone(), msg.cc.clone(), msg.date.map(full_date).unwrap_or_default()),
            };
            line(ui, "From", &from);
            line(ui, "To", &to);
            line(ui, "Cc", &cc);
            line(ui, "Date", &date);
            line(ui, "Folder", &format!("{} ({})", folder_label_of(&msg.folder), cfg.mail_accounts.iter().find(|a| a.id == msg.account).map(|a| a.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| msg.account.clone())));
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                let can = msg.state == STATE_FULL;
                let r = ui.add_enabled(can || !self.marked.is_empty(), egui::Button::new("Import to vault"));
                rec(rects, "mail:import", &r);
                let r = if can {
                    r.on_hover_text("Make a note of this mail (or of the marked ones) in the Import dialog; nothing is written until you press Import there (i)")
                } else {
                    r.on_disabled_hover_text("Load the message first: it was not downloaded")
                };
                if r.clicked() {
                    import = true;
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
                // answering needs the message text (MAIL-126)
                let ready = msg.state == STATE_FULL;
                for (key, label, kind, tip) in [
                    ("mail:reply", "Reply", Kind::Reply, "Answer the sender (r)"),
                    ("mail:reply-all", "Reply all", Kind::ReplyAll, "Answer everyone (Shift+R)"),
                    ("mail:forward", "Forward", Kind::Forward, "Forward with the attachments (Shift+F)"),
                ] {
                    let r = ui.add_enabled(ready, egui::Button::new(label));
                    rec(rects, key, &r);
                    let r = if ready { r.on_hover_text(tip) } else { r.on_disabled_hover_text("Load the message first") };
                    if r.clicked() {
                        answer = Some(kind);
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                let n = if self.marked.is_empty() { String::new() } else { format!(" {}", self.marked.len()) };
                let live = !msg.gone_on_server;
                let why = "This mail is gone from the server";
                for (key, label, a, tip) in [
                    ("mail:read-toggle", if all_read { format!("Mark{n} unread") } else { format!("Mark{n} read") }, Act::Read, "Mark as read or unread on the server (u)"),
                    ("mail:flag-toggle", if all_flagged { format!("Unflag{n}") } else { format!("Flag{n}") }, Act::Flag, "Set or clear the flag on the server (s)"),
                    ("mail:move", format!("Move{n}…"), Act::Move, "Move to another folder: type to find it (m)"),
                    ("mail:delete", if all_trash { format!("Delete{n} forever…") } else { format!("Delete{n}") }, Act::Delete, if all_trash { "Remove from Trash for good, after you confirm (d)" } else { "Move to Trash (d)" }),
                ] {
                    let r = ui.add_enabled(live, egui::Button::new(label));
                    rec(rects, key, &r);
                    let r = if live { r.on_hover_text(tip) } else { r.on_disabled_hover_text(why) };
                    if r.clicked() {
                        act = Some(a);
                    }
                }
                if msg.is_flagged() {
                    ui.label(RichText::new("Flagged").size(12.0).color(theme::LINK_MISSING));
                }
                if let Some(k) = &msg.pending {
                    ui.label(muted(format!("{} waiting for the server", pending_word(k).trim_end_matches('…'))).size(12.0));
                }
            });
            if msg.gone_on_server {
                ui.add_space(4.0);
                ui.label(RichText::new("This mail was deleted on the server. This is the copy Shale kept.").color(theme::LINK_MISSING).size(12.5));
            }
            ui.separator();
            match &self.open.as_ref().map(|o| &o.body) {
                Some(Body::Loading) => {
                    ui.label(muted("Reading the message…"));
                }
                Some(Body::Fetching) => {
                    ui.label(muted("Downloading the message from the server…"));
                }
                Some(Body::Failed(e)) => {
                    ui.label(RichText::new(e.as_str()).color(theme::LINK_MISSING));
                    if e.contains(NO_PASSWORD) {
                        ui.label(muted("Type the password above, or save one in the account settings."));
                    } else if crate::oauth::needs_sign_in(e) {
                        ui.label(muted("Open the account settings and sign in again."));
                    }
                }
                Some(Body::NeedsDownload) | Some(Body::TooLarge) => {
                    let why = if matches!(self.open.as_ref().map(|o| &o.body), Some(Body::TooLarge)) {
                        format!("This message is {} and over the limit of {}, so it was not downloaded with the others.", size_text(msg.size), size_text(self.opts.limits.max_message))
                    } else {
                        "Only the headers of this message are here: mail in Trash and Junk is not synced, a message is downloaded when you open it.".to_string()
                    };
                    ui.label(muted(why));
                    ui.add_space(6.0);
                    let r = ui.button("Load this message");
                    rec(rects, "mail:load", &r);
                    if r.clicked() {
                        load = true;
                    }
                }
                Some(Body::Ready(l)) => {
                    let l = l.clone();
                    security_strip(ui, &l.security, rects, &mut strip);
                    if !l.mail.attachments.is_empty() {
                        ui.label(muted(format!("{} attachment{}", l.mail.attachments.len(), if l.mail.attachments.len() == 1 { "" } else { "s" })).size(12.5));
                        for (i, a) in l.mail.attachments.iter().enumerate() {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(if a.name.is_empty() { "(unnamed)" } else { a.name.as_str() }).color(theme::TEXT));
                                ui.label(muted(size_text(a.data.len() as u64)).size(12.0));
                                let o = ui.small_button("Open");
                                rec(rects, format!("mail:att-open:{i}"), &o);
                                if o.on_hover_text("Open a temporary copy with the default program").clicked() {
                                    open_att = Some(i);
                                }
                                let s = ui.small_button("Save to vault");
                                rec(rects, format!("mail:att-save:{i}"), &s);
                                if s.clicked() {
                                    save_att = Some(i);
                                }
                            });
                        }
                        ui.separator();
                    }
                    ui.add(egui::Label::new(RichText::new(l.shown.as_str()).color(theme::TEXT).size(cfg.font_size - 1.0)).selectable(true));
                    if l.cut {
                        ui.add_space(6.0);
                        ui.label(muted("The text is very long and is cut here. Import the mail to get all of it."));
                    }
                    if !l.mail.attachments.is_empty() || l.shown.trim().is_empty() {
                        // nothing more
                    }
                }
                None => {}
            }
        });
        if strip.cancel {
            self.cancel_crypto();
        }
        if strip.settings {
            actions.push(MailAction::OpenAccounts);
        }
        if let Some(k) = answer {
            self.start_reply(k);
        }
        if load {
            self.load_message(cfg, &msg);
        }
        if let Some(a) = act {
            self.run_act(cfg, a, targets);
        }
        if import {
            let t = self.import_targets();
            if !t.is_empty() {
                actions.push(MailAction::Import(t));
            }
        }
        if close {
            self.close_message();
        }
        if let Some(Body::Ready(l)) = self.open.as_ref().map(|o| &o.body) {
            if let Some(i) = open_att {
                if let Some(a) = l.mail.attachments.get(i) {
                    if let Err(e) = crate::opener::open_temp_copy(&a.name, &a.data) {
                        actions.push(MailAction::Toast(e));
                    }
                }
            }
            if let Some(index) = save_att {
                actions.push(MailAction::SaveAttachment { loaded: l.clone(), index });
            }
        }
    }

    // ------------------------------------------------------------ the folder picker and the confirmation

    /// `m`: type to find a folder, ↑ ↓ to choose, Enter to move, Esc to leave (MAIL-110).
    fn mover_window(&mut self, ui: &mut Ui, cfg: &Config, rects: &mut Vec<(String, Rect)>) {
        if self.mover.is_none() {
            return;
        }
        let (up, down, enter, esc) = ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape)));
        if esc {
            self.mover = None;
            return;
        }
        let mut cands = self.mover_candidates();
        if let Some(p) = self.mover.as_mut() {
            if down && !cands.is_empty() {
                p.cursor = (p.cursor + 1).min(cands.len() - 1);
            }
            if up {
                p.cursor = p.cursor.saturating_sub(1);
            }
            p.cursor = p.cursor.min(cands.len().saturating_sub(1));
        }
        if enter {
            let at = self.mover.as_ref().map_or(0, |p| p.cursor);
            if let Some(f) = cands.get(at).cloned() {
                self.move_to(cfg, &f);
            }
            return;
        }
        let mut chosen: Option<FolderRowPick> = None;
        let mut cancel = false;
        let count = self.mover.as_ref().map_or(0, |p| p.msgs.len());
        let mut changed = false;
        let title = format!("Move {count} message{} to…", if count == 1 { "" } else { "s" });
        let resp = crate::dialog::Dialog::new("mail-mover", title).width(340.0).non_modal().show(ui.ctx(), |ui| {
            if let Some(p) = self.mover.as_mut() {
                let id = egui::Id::unique("mail-mover-field");
                let r = ui.add(egui::TextEdit::singleline(&mut p.query).id(id).hint_text("Type to find a folder (↑ ↓ Enter, Esc)").desired_width(f32::INFINITY));
                rec(rects, "mail:mover-field", &r);
                if std::mem::take(&mut p.focus) {
                    r.request_focus();
                }
                changed = r.changed();
            }
            ui.add_space(4.0);
            let at = self.mover.as_ref().map_or(0, |p| p.cursor);
            egui::ScrollArea::vertical().max_height(260.0).id_salt("mail-mover-list").show(ui, |ui| {
                if cands.is_empty() {
                    ui.label(muted("No folder of that name."));
                }
                for (k, f) in cands.iter().enumerate() {
                    let label = crate::imap_client::decode_mutf7(&f.name);
                    let r = ui.selectable_label(k == at, label);
                    rec(rects, format!("mail:mover-item:{k}"), &r);
                    if r.clicked() {
                        chosen = Some(FolderRowPick(k));
                    }
                }
            });
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
        cancel |= resp.should_close();
        if changed {
            if let Some(p) = self.mover.as_mut() {
                p.cursor = 0;
            }
        }
        if cancel {
            self.mover = None;
        } else if let Some(FolderRowPick(k)) = chosen {
            if k < cands.len() {
                let f = cands.swap_remove(k);
                self.move_to(cfg, &f);
            }
        }
    }

    /// "Delete forever?": nothing happens until the button is pressed (Enter does not confirm).
    fn forever_window(&mut self, ui: &mut Ui, cfg: &Config, rects: &mut Vec<(String, Rect)>) {
        let Some(n) = self.forever.as_ref().map(|a| a.msgs.len()) else { return };
        if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.forever = None;
            return;
        }
        let (mut yes, mut no) = (false, false);
        let title = format!("Delete {n} message{} forever?", if n == 1 { "" } else { "s" });
        let resp = crate::dialog::Dialog::new("mail-forever", title).width(380.0).non_modal().show(ui.ctx(), |ui| {
            ui.label(muted("They are removed from the server and cannot be brought back from there. Shale keeps its own local copy of each, in the folder “.trash” of its mail folder.").size(12.5));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let r = ui.button(RichText::new(format!("Delete {n} forever")).color(theme::LINK_MISSING));
                rec(rects, "mail:forever-confirm", &r);
                if r.clicked() {
                    yes = true;
                }
                let r = ui.button("Cancel");
                rec(rects, "mail:forever-cancel", &r);
                if r.clicked() {
                    no = true;
                }
            });
        });
        no |= resp.should_close();
        if yes {
            self.confirm_forever(cfg);
        } else if no {
            self.forever = None;
        }
    }
}

/// The index of the folder clicked in the picker.
struct FolderRowPick(usize);

/// A tiny paper clip, drawn with lines (the default fonts have no such symbol).
/// What the status strip asked for.
#[derive(Default)]
struct StripOut {
    cancel: bool,
    settings: bool,
}

/// The status strip above a signed or encrypted mail (MAIL-174): built from GnuPG's verdicts only, in its own widget and colours,
/// never from anything in the mail. Each line is a framed label; a body text can not be drawn in here.
fn security_strip(ui: &mut Ui, sec: &crate::mail_crypto::Security, rects: &mut Vec<(String, Rect)>, out: &mut StripOut) {
    use crate::mail_crypto::{Encryption, Level};
    if sec.is_empty() {
        return;
    }
    for (i, (level, text)) in sec.describe().into_iter().enumerate() {
        let (color, fill) = match level {
            Level::Good => (Color32::from_rgb(110, 205, 130), Color32::from_rgb(30, 52, 38)),
            Level::Warn => (Color32::from_rgb(235, 185, 85), Color32::from_rgb(58, 48, 28)),
            Level::Bad => (theme::LINK_MISSING, Color32::from_rgb(66, 32, 32)),
            Level::Info => (theme::MUTED, theme::BG_SECONDARY),
        };
        let r = egui::Frame::new().fill(fill).stroke(Stroke::new(1.0, color)).corner_radius(CornerRadius::same(4)).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).color(color).size(12.5));
        });
        rects.push((format!("mail:sec:{i}"), r.response.rect));
        ui.add_space(3.0);
    }
    ui.horizontal(|ui| {
        if sec.working.is_some() {
            let b = ui.button("Cancel");
            rec(rects, "mail:sec:cancel", &b);
            if b.on_hover_text("Stop waiting for GnuPG (a passphrase dialog may still be open)").clicked() {
                out.cancel = true;
            }
        }
        if sec.encryption == Encryption::NeedsConsent {
            let b = ui.button("Encryption settings…");
            rec(rects, "mail:sec:settings", &b);
            if b.clicked() {
                out.settings = true;
            }
        }
    });
    ui.add_space(4.0);
}

fn paint_clip(p: &egui::Painter, c: Pos2, color: Color32) {
    let s = Stroke::new(1.2, color);
    let r = Rect::from_center_size(c, Vec2::new(7.0, 13.0));
    p.rect_stroke(r, CornerRadius::same(3), s, egui::StrokeKind::Middle);
    p.line_segment([Pos2::new(c.x, c.y - 3.0), Pos2::new(c.x, c.y + 3.0)], s);
}
