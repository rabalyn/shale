//! How an entry of the Notifications feed is drawn (SOC-071): a row above the card that says what
//! happened and who did it, with the age and a dot while it is unread; a notification about a
//! person (a follow) shows only the person and "Open in browser".

use super::feed_ui::{age_text, open_web, Cx};
use super::notifications::Notice;
use super::{Network, Post};
use crate::theme;
use eframe::egui::{self, RichText, Sense, Ui, Vec2};

/// The row: the mark of the kind, "Ada and Bo favourited your post", the age, and a dot while unread.
pub(super) fn notice_row(ui: &mut Ui, cx: &mut Cx, n: &Notice, network: Network, idx: usize) {
    let pal = theme::palette();
    let r = ui
        .horizontal(|ui| {
            ui.label(RichText::new(n.kind.icon()).size(crate::fonts::px(13.0)).color(pal.accent));
            let line = n.line(network);
            ui.add(egui::Label::new(RichText::new(&line).size(crate::fonts::px(13.0)).color(pal.text)).truncate()).on_hover_text(line);
            let age = age_text(n.at, cx.now);
            if !age.is_empty() {
                ui.label(RichText::new(format!("· {age}")).size(crate::fonts::px(12.0)).color(pal.muted)).on_hover_text(crate::import::local_stamp(n.at, false));
            }
            if n.unread {
                let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 4.0, pal.accent);
                cx.rects.push((format!("unread:{idx}"), dot));
            }
        })
        .response;
    cx.rec(format!("notice:{idx}"), r);
    ui.add_space(4.0);
}

/// What a card about a person has below its header: "Open in browser".
pub(super) fn person_actions(ui: &mut Ui, cx: &mut Cx, p: &Post, idx: usize) {
    ui.add_space(6.0);
    if cx.rec(format!("open:{idx}"), ui.button("Open in browser")).on_hover_text("Open the profile of this account").clicked() {
        open_web(&p.url);
    }
}
