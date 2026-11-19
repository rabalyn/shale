//! Local sent copies that the server never got (MAIL-210…MAIL-214): the copy of a sent mail is kept
//! locally and an `append` uploads it to the server's Sent folder. When that queue row is gone without
//! the upload (the index was restored from a backup, MAIL-205, or the server refused), the copy would stay
//! on this computer silently. Here the person is told, and can upload it again or keep it as it is.
//! Nothing in this flow deletes a message row or a file.

use crate::app::muted;
use crate::config::Config;
use crate::keys::Action as KeyAction;
use crate::mail::db::{Op, UnsentCopy};
use crate::mail::ui::*;
use crate::theme;
use eframe::egui::{self, Align, Layout, Rect, RichText, Ui, UiBuilder, Vec2};

/// "3 sent mails are only on this computer".
pub fn unsent_text(n: usize) -> String {
    format!("{n} sent mail{} {} only on this computer", if n == 1 { "" } else { "s" }, if n == 1 { "is" } else { "are" })
}

fn rec(rects: &mut Vec<(String, Rect)>, key: impl Into<String>, r: &egui::Response) {
    rects.push((key.into(), r.rect));
}

impl MailView {
    /// Is this account still set up? Only then can its copy be uploaded (MAIL-213).
    fn unsent_can_upload(cfg: &Config, c: &UnsentCopy) -> bool {
        cfg.mail_accounts.iter().any(|a| a.id == c.account)
    }

    /// Queue an `append` for each of the copies, through the normal queue (MAIL-211). Returns how many were asked for.
    pub fn unsent_upload(&mut self, cfg: &Config, which: &[UnsentCopy]) -> usize {
        let Some(store) = self.ready_store() else { return 0 };
        let now = crate::calendar::time::now();
        let ok: Vec<&UnsentCopy> = which.iter().filter(|c| Self::unsent_can_upload(cfg, c)).collect();
        // the check "nothing is queued for it" is part of the operation itself: a second click queues nothing
        if let Err(e) = store.commit(ok.iter().map(|c| Op::QueueAppend { folder: c.folder, uid: c.uid, created: now }).collect()) {
            self.problems.push(format!("The sent mail could not be queued for upload: {e}"));
            return 0;
        }
        self.refresh();
        let mut accounts: Vec<&str> = ok.iter().map(|c| c.account.as_str()).collect();
        accounts.sort_unstable();
        accounts.dedup();
        for a in accounts {
            self.start_ops(cfg, a);
        }
        ok.len()
    }

    /// Keep the copies as they are and stop showing them (MAIL-212).
    pub fn unsent_dismiss(&mut self, which: &[UnsentCopy]) {
        let Some(store) = self.ready_store() else { return };
        if let Err(e) = store.commit(which.iter().map(|c| Op::DismissUnsent { folder: c.folder, uid: c.uid }).collect()) {
            self.problems.push(format!("Could not remember that: {e}"));
            return;
        }
        self.refresh();
    }

    /// The notice above the list.
    pub(crate) fn unsent_strip(&mut self, ui: &mut Ui, body: &mut Rect, rects: &mut Vec<(String, Rect)>) {
        if self.unsent.is_empty() {
            return;
        }
        let strip = Rect::from_min_size(body.min, Vec2::new(body.width(), 30.0));
        let text = unsent_text(self.unsent.len());
        let mut open = false;
        let show_tip = self.tip("Show these mails", KeyAction::MailUnsent);
        ui.scope_builder(UiBuilder::new().max_rect(strip).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.set_clip_rect(strip.intersect(ui.clip_rect()));
            ui.add_space(10.0);
            let l = ui.label(RichText::new(text).size(12.5).color(theme::TEXT));
            rec(rects, "mail:unsent-notice", &l);
            let r = ui.small_button("Show…");
            rec(rects, "mail:unsent-open", &r);
            open = r.on_hover_text(show_tip.as_str()).clicked();
        });
        body.min.y = strip.max.y;
        if open {
            self.unsent_open = true;
        }
    }

    /// The list of them, with "Upload again" and "Dismiss".
    pub(crate) fn unsent_window(&mut self, ui: &mut Ui, cfg: &Config, rects: &mut Vec<(String, Rect)>) {
        if !self.unsent_open || self.unsent.is_empty() {
            return;
        }
        let list = self.unsent.clone();
        let (mut upload, mut dismiss): (Option<Vec<UnsentCopy>>, Option<Vec<UnsentCopy>>) = (None, None);
        let mut close = false;
        let dismiss_tip = self.tip("Keep the mail in the local Sent folder and stop showing it here", KeyAction::MailDelete);
        let close_tip = self.tip("Close this window", KeyAction::MailUnsent);
        let resp = crate::dialog::Dialog::new("mail-unsent", "Sent mails only on this computer").width(620.0).non_modal().show(ui.ctx(), |ui| {
            crate::theme::prose(ui, muted("The server's Sent folder does not have these. “Upload again” sends a copy there; “Dismiss” keeps the mail in the local Sent folder and stops showing it here. Nothing is deleted.").size(12.5));
            egui::ScrollArea::vertical().max_height(300.0).id_salt("mail-unsent-list").show(ui, |ui| {
                let sel = self.kbd.unsent_sel.min(list.len() - 1);
                for (k, c) in list.iter().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        let subject = if c.subject.trim().is_empty() { "(no subject)" } else { c.subject.as_str() };
                        // the mail that Enter and d / Delete act on (MAIL-069)
                        if k == sel {
                            let m = ui.label(RichText::new("▸").strong().color(theme::ACCENT));
                            rec(rects, "mail:unsent-sel", &m);
                            m.scroll_to_me(None);
                        }
                        ui.label(RichText::new(subject).strong());
                        let when = c.date.map(full_date).unwrap_or_default();
                        let name = self.account_names.get(&c.account).cloned().unwrap_or_else(|| c.account.clone());
                        crate::theme::hint(ui, format!("to {} · {when} · {name}", c.to_addr));
                    });
                    ui.horizontal(|ui| {
                        if Self::unsent_can_upload(cfg, c) {
                            let r = ui.small_button("Upload again");
                            rec(rects, format!("mail:unsent-upload:{k}"), &r);
                            if r.clicked() {
                                upload = Some(vec![c.clone()]);
                            }
                        } else {
                            crate::theme::hint(ui, "This account is not set up any more, so it cannot be uploaded.");
                        }
                        let r = ui.small_button("Dismiss");
                        rec(rects, format!("mail:unsent-dismiss:{k}"), &r);
                        if r.on_hover_text(dismiss_tip.as_str()).clicked() {
                            dismiss = Some(vec![c.clone()]);
                        }
                    });
                    ui.separator();
                }
            });
            ui.horizontal(|ui| {
                if list.iter().any(|c| Self::unsent_can_upload(cfg, c)) {
                    let r = ui.button("Upload all again");
                    rec(rects, "mail:unsent-upload-all", &r);
                    if r.clicked() {
                        upload = Some(list.clone());
                    }
                }
                let r = ui.button("Dismiss all");
                rec(rects, "mail:unsent-dismiss-all", &r);
                if r.clicked() {
                    dismiss = Some(list.clone());
                }
                let r = ui.button("Close");
                rec(rects, "mail:unsent-close", &r);
                close = r.on_hover_text(close_tip.as_str()).clicked();
            });
        });
        if let Some(w) = upload {
            self.unsent_upload(cfg, &w);
        } else if let Some(w) = dismiss {
            self.unsent_dismiss(&w);
        }
        if close || resp.should_close() {
            self.unsent_open = false;
        }
    }
}
