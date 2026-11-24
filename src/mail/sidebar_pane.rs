//! Mail as a view of the right sidebar (LAY-028…LAY-032, ADR-0038): a compact companion of the
//! Mail tab, not the three panes squeezed into a narrow column. It has the accounts, the folders
//! of the chosen account and the list of its messages with their unread state; a click on a
//! message asks the window to open it in the Mail tab.
//!
//! The view state (the chosen account and folder, the selected message, the scroll of the list)
//! is the pane's own, so the sidebar can show the Inbox while the tab shows another folder
//! (LAY-031). The data is the tab's: the index and the folder counts of the one `MailView`, which
//! also syncs, so nothing is read or fetched twice. The pages of the list are the pane's own and
//! are read again when the index was read again (`MailView::refreshed`).

use crate::app::muted;
use crate::config::Config;
use crate::mail::db::MessageSummary;
use crate::mail::ui::{folder_label, MailView, StoreState, PAGE};
use crate::mail::view::{paint_message_row, RowState, ROW_H};
use crate::theme;
use eframe::egui::{self, Rect, RichText, Sense, Ui, Vec2};
use std::collections::HashMap;

/// The longest the list of folders gets before it scrolls, in points.
const FOLDERS_MAX_H: f32 = 120.0;

/// The state of the Mail view of the right sidebar. It is not saved: a new window opens at the
/// first folder of the first account.
#[derive(Default)]
pub(crate) struct MailPane {
    /// the chosen account (an id); None: the first one
    pub account: Option<String>,
    /// the listed folder; None until the account's first folder is chosen
    pub folder: Option<i64>,
    /// the message the person clicked last
    pub selected: Option<i64>,
    /// how far the list is scrolled, in points (the last frame's)
    pub scroll: f32,
    total: usize,
    pages: HashMap<usize, Vec<MessageSummary>>,
    /// the `MailView::refreshed` the pages and the total are from
    read_at: u64,
    /// controls drawn in the last frame, by name (for the UI tests)
    pub rects: Vec<(String, Rect)>,
}

impl MailPane {
    /// Draw the pane into the free space of `ui`. Returns the message that was clicked: the
    /// window opens it in the Mail tab.
    pub fn show(&mut self, ui: &mut Ui, mail: &MailView, cfg: &Config) -> Option<MessageSummary> {
        self.rects.clear();
        if cfg.mail_accounts.is_empty() {
            ui.label(muted("No mail account yet. Choose Mail in the ribbon and add one in its settings."));
            return None;
        }
        if let StoreState::Failed(e) = &mail.store {
            ui.label(RichText::new(format!("The mail index could not be opened: {e}")).color(theme::palette().link_missing));
            return None;
        }
        let Some(reader) = mail.reader() else {
            ui.label(muted("Opening the mail index…"));
            return None;
        };
        let mut rects = std::mem::take(&mut self.rects);
        // the account: the chosen one while it exists, else the first
        let valid = |id: &Option<String>| id.clone().filter(|id| cfg.mail_accounts.iter().any(|a| a.id == *id));
        if cfg.mail_accounts.len() > 1 {
            let current = valid(&self.account).unwrap_or_else(|| cfg.mail_accounts[0].id.clone());
            ui.horizontal_wrapped(|ui| {
                for a in &cfg.mail_accounts {
                    let title = if a.name.is_empty() { a.host.clone() } else { a.name.clone() };
                    let r = ui.selectable_label(a.id == current, title);
                    rects.push((format!("mailpane:account:{}", a.id), r.rect));
                    if r.clicked() {
                        self.account = Some(a.id.clone());
                    }
                }
            });
        }
        let account = valid(&self.account).unwrap_or_else(|| cfg.mail_accounts[0].id.clone());
        let folders = mail.account_folders(&account);
        // a folder of another account, or none: the first folder (the Inbox) of this one
        if !self.folder.is_some_and(|f| folders.iter().any(|r| r.id == f)) {
            self.folder = folders.first().map(|f| f.id);
            self.selected = None;
            self.pages.clear();
            self.total = self.folder.and_then(|f| reader.count_folder(f).ok()).unwrap_or(0).max(0) as usize;
            self.read_at = mail.refreshed;
        }
        let mut chosen = None;
        egui::ScrollArea::vertical().id_salt("mail-pane-folders").max_height(FOLDERS_MAX_H).auto_shrink([false, true]).show(ui, |ui| {
            if folders.is_empty() {
                ui.label(muted("No folders yet. Sync in the Mail tab."));
            }
            for f in &folders {
                let unread = mail.counts.get(&f.id).map_or(0, |c| c.1);
                let (depth, label) = mail.tree_entry(f);
                let text = if unread > 0 { RichText::new(format!("{label}  ({unread})")).strong().color(theme::palette().text_strong) } else { RichText::new(label).color(theme::palette().text) };
                ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * 12.0);
                    let r = ui.selectable_label(self.folder == Some(f.id), text);
                    rects.push((format!("mailpane:folder:{}", f.id), r.rect));
                    if r.clicked() {
                        chosen = Some(f.id);
                    }
                });
            }
        });
        if let Some(id) = chosen.filter(|id| self.folder != Some(*id)) {
            self.folder = Some(id);
            self.selected = None;
            self.pages.clear();
            self.total = reader.count_folder(id).unwrap_or(0).max(0) as usize;
            self.read_at = mail.refreshed;
        }
        // the index was read again (mail arrived, a flag changed): the pages are old
        if self.read_at != mail.refreshed {
            self.read_at = mail.refreshed;
            self.pages.clear();
            self.total = self.folder.and_then(|f| reader.count_folder(f).ok()).unwrap_or(0).max(0) as usize;
        }
        ui.separator();
        let Some(folder) = self.folder else {
            self.rects = rects;
            return None;
        };
        if let Some(f) = folders.iter().find(|f| f.id == folder) {
            ui.label(muted(format!("{}  ·  {} message{}", folder_label(f), self.total, if self.total == 1 { "" } else { "s" })));
        }
        if self.total == 0 {
            ui.label(muted("No messages here."));
            self.rects = rects;
            return None;
        }
        let mut clicked = None;
        let now = crate::calendar::time::now();
        ui.spacing_mut().item_spacing.y = 0.0;
        let out = egui::ScrollArea::vertical().id_salt("mail-pane-list").auto_shrink([false, false]).show_rows(ui, ROW_H, self.total, |ui, range| {
            for page in range.start / PAGE..=(range.end.max(1) - 1) / PAGE {
                self.pages.entry(page).or_insert_with(|| reader.list(folder, PAGE, page * PAGE).unwrap_or_default());
            }
            for i in range {
                let Some(m) = self.pages.get(&(i / PAGE)).and_then(|p| p.get(i % PAGE)).cloned() else {
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
                    continue;
                };
                let state = RowState { cursor: self.selected == Some(m.id), opened: mail.open.as_ref().is_some_and(|o| o.msg.id == m.id), ..RowState::default() };
                let r = paint_message_row(ui, i, &m, state, now, &mut rects);
                rects.push((format!("mailpane:row:{i}"), r.rect));
                if r.clicked() {
                    clicked = Some(m);
                }
            }
        });
        self.scroll = out.state.clamped_offset().y;
        self.rects = rects;
        if let Some(m) = &clicked {
            self.selected = Some(m.id);
        }
        clicked
    }
}
