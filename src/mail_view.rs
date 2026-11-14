//! Drawing the mail view and its keys (ADR-0007 phase 1b; MAIL-050…MAIL-061).
//!
//! Three panes: accounts and folders, the message list, the reading pane. The list is drawn with
//! `show_rows`: only the rows on screen are laid out, and only their pages are read from the
//! index, so a steady frame costs no query. The reading pane shows plain text only.

use crate::app::muted;
use crate::config::Config;
use crate::imap_sync::{FolderKind, NO_PASSWORD};
use crate::mail::display_name;
use crate::mail_db::{MessageSummary, STATE_FULL, STATE_HEADER, STATE_TOO_LARGE};
use crate::mail_ui::*;
use crate::theme;
use eframe::egui::{self, Align, Color32, CornerRadius, FontId, Key, Layout, Modifiers, Pos2, Rect, RichText, Sense, Stroke, Ui, UiBuilder, Vec2};

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
                    ui.label(muted("Shale reads your mail from your mail server and keeps a local copy. It never changes anything on the server."));
                    ui.add_space(8.0);
                    let r = ui.button("Add an account…");
                    rec(&mut rects, "mail:add-account", &r);
                    if r.clicked() {
                        actions.push(MailAction::OpenAccounts);
                    }
                });
            }
            StoreState::Ready(_) => self.panes(ui, cfg, body, &mut actions, &mut rects),
        }
        self.rects = rects;
        actions
    }

    // ------------------------------------------------------------ keys

    fn handle_keys(&mut self, ui: &mut Ui, cfg: &Config, modal_open: bool, actions: &mut Vec<MailAction>) {
        let ctx = ui.ctx().clone();
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
        if modal_open || ctx.memory(|m| m.focused().is_some()) {
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
            let r = ui.add(egui::TextEdit::singleline(&mut p.text).password(true).desired_width(200.0));
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
                            let r = ui.selectable_label(self.folder == Some(f.id), text);
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
                });
            }
        });
        if let Some(id) = chosen {
            self.select_folder(cfg, id);
        }
    }

    fn list_pane(&mut self, ui: &mut Ui, cfg: &Config, actions: &mut Vec<MailAction>, rects: &mut Vec<(String, Rect)>) {
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
        let now = crate::cal_time::now();
        ui.spacing_mut().item_spacing.y = 0.0;
        let out = area.show_rows(ui, ROW_H, n, |ui, range| {
            self.ensure_rows(range.clone());
            for i in range.clone() {
                let Some(m) = self.row(i).cloned() else {
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
                    continue;
                };
                let r = self.paint_row(ui, i, &m, now);
                rec(rects, format!("mail:row:{i}"), &r);
                if r.clicked() {
                    clicked = Some((i, ui.input(|i| i.modifiers.command)));
                }
            }
            range
        });
        self.visible = out.inner;
        self.list_h = out.inner_rect.height();
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

    fn paint_row(&self, ui: &mut Ui, i: usize, m: &MessageSummary, now: i64) -> egui::Response {
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
        let dim = if m.gone_on_server { 0.45 } else { 1.0 };
        let unread = !m.is_seen() && m.state != STATE_TOO_LARGE;
        let strong = |c: Color32| c.gamma_multiply(dim);
        let from = if m.from.trim().is_empty() { "(unknown sender)".to_string() } else { display_name(&m.from) };
        let date = list_date(m.date, now);
        let date_g = p.layout_no_wrap(date, FontId::proportional(12.0), strong(theme::MUTED));
        let x0 = rect.min.x + 16.0;
        let date_x = rect.max.x - 10.0 - date_g.size().x;
        if unread {
            p.circle_filled(Pos2::new(rect.min.x + 8.0, rect.min.y + 14.0), 3.0, theme::ACCENT);
        }
        p.galley(Pos2::new(date_x, rect.min.y + 7.0), date_g, Color32::WHITE);
        let from_g = p.layout_no_wrap(from, FontId::proportional(14.0), strong(if unread { theme::TEXT_STRONG } else { theme::TEXT }));
        p.with_clip_rect(Rect::from_min_max(Pos2::new(x0, rect.min.y), Pos2::new(date_x - 8.0, rect.max.y))).galley(Pos2::new(x0, rect.min.y + 6.0), from_g, Color32::WHITE);
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
        let subj_g = p.layout_no_wrap(subject, FontId::proportional(13.0), strong(if unread { theme::TEXT } else { theme::MUTED }));
        p.with_clip_rect(Rect::from_min_max(Pos2::new(x0, rect.min.y), Pos2::new(right_edge, rect.max.y))).galley(Pos2::new(x0, rect.min.y + 25.0), subj_g, Color32::WHITE);
        resp
    }

    fn reading_pane(&mut self, ui: &mut Ui, cfg: &Config, actions: &mut Vec<MailAction>, rects: &mut Vec<(String, Rect)>) {
        let Some(open) = self.open.as_ref() else {
            ui.add_space(30.0);
            ui.label(muted("Select a message to read it (Enter opens the one under the cursor)."));
            return;
        };
        let msg = open.msg.clone();
        let mut load = false;
        let mut import = false;
        let mut close = false;
        let mut open_att: Option<usize> = None;
        let mut save_att: Option<usize> = None;
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
        if load {
            self.load_message(cfg, &msg);
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
}

/// A tiny paper clip, drawn with lines (the default fonts have no such symbol).
fn paint_clip(p: &egui::Painter, c: Pos2, color: Color32) {
    let s = Stroke::new(1.2, color);
    let r = Rect::from_center_size(c, Vec2::new(7.0, 13.0));
    p.rect_stroke(r, CornerRadius::same(3), s, egui::StrokeKind::Middle);
    p.line_segment([Pos2::new(c.x, c.y - 3.0), Pos2::new(c.x, c.y + 3.0)], s);
}
