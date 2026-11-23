//! Scheduled posts on Mastodon (SOC-063, SOC-064): the date and time typed in the compose box
//! (local time, at least 5 minutes ahead), the answer of the server (a `ScheduledStatus`, not a
//! post), and the list of the scheduled posts with Cancel. Bluesky has no scheduling.

use super::actions::{busy_key_of, ActionError};
use super::feed_ui::{short, TabCtx};
use super::state::{run, Msg, SocialState};
use super::*;
use crate::calendar::time as t;
use eframe::egui::{self, Context, Rect, RichText, Vec2};

/// Mastodon refuses a time less than 5 minutes ahead; a little more is asked so that the
/// minutes that pass while the box is open cannot make it too late (SOC-063).
pub const MIN_AHEAD: i64 = 5 * 60 + 30;

/// A post the server holds back until its time.
#[derive(Clone, Debug, PartialEq)]
pub struct Scheduled {
    pub id: String,
    /// seconds since 1970 (UTC)
    pub at: i64,
    pub text: String,
    /// the word the API uses ("public" …)
    pub visibility: String,
    pub media: usize,
    pub poll: bool,
}

impl Scheduled {
    /// From the JSON of a Mastodon `ScheduledStatus` (None without an id or a time).
    pub fn parse(v: &Value) -> Option<Scheduled> {
        let (id, at) = (text(&v["id"]), parse_rfc3339(&text(&v["scheduled_at"]))?);
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        let params = &v["params"];
        Some(Scheduled { id, at, text: text(&params["text"]), visibility: text(&params["visibility"]), media: v["media_attachments"].as_array().map_or(0, Vec::len), poll: params["poll"].is_object() })
    }

    /// "Mon, 23 Nov 2026 14:30" in local time.
    pub fn when(&self) -> String {
        when_text(self.at)
    }
}

/// A moment as the box and the list name it: local time with the weekday.
pub fn when_text(at: i64) -> String {
    let (days, secs) = t::utc_to_local(at);
    format!("{} {}", t::long_date(days), t::hhmm(secs))
}

/// The date and time typed for a scheduled post (local time).
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduleInput {
    /// "2026-11-23"
    pub date: String,
    /// "14:30"
    pub time: String,
}

impl ScheduleInput {
    /// An hour from now, rounded up to the next quarter of an hour.
    pub fn soon() -> ScheduleInput {
        ScheduleInput::at(t::now() + 3600 + 899)
    }

    /// The input that names this moment.
    pub fn at(utc: i64) -> ScheduleInput {
        let (days, secs) = t::utc_to_local(utc);
        let (y, m, d) = t::civil_from_days(days);
        ScheduleInput { date: format!("{y:04}-{m:02}-{d:02}"), time: t::hhmm(secs / 900 * 900) }
    }

    /// The moment (UTC seconds), or why the input is no usable time.
    pub fn parse(&self, now: i64) -> Result<i64, String> {
        let days = t::parse_ymd(self.date.trim()).ok_or("Type the date as year-month-day, for example 2026-11-23.")?;
        let (h, m) = self
            .time
            .trim()
            .split_once(':')
            .and_then(|(h, m)| Some((h.trim().parse::<i64>().ok()?, m.trim().parse::<i64>().ok()?)))
            .filter(|(h, m)| (0..24).contains(h) && (0..60).contains(m))
            .ok_or("Type the time as hours:minutes, for example 14:30.")?;
        let at = t::local_to_utc(days, h * 3600 + m * 60);
        if at < now + MIN_AHEAD {
            return Err("A scheduled post must be at least 5 minutes ahead.".into());
        }
        Ok(at)
    }
}

/// What the scheduled-posts window shows.
#[derive(Default)]
pub struct ScheduledState {
    /// the window is open over this tab
    pub tab: Option<u64>,
    pub account: String,
    pub loading: bool,
    pub items: Vec<Scheduled>,
    pub error: Option<String>,
}

impl SocialState {
    /// Open the window of the scheduled posts over a tab and ask the server for them.
    pub fn open_scheduled(&mut self, ctx: &Context, e: &SocialAccountEntry, tab: u64) {
        if e.account.network != Network::Mastodon {
            return;
        }
        self.scheduled = ScheduledState { tab: Some(tab), account: e.account.id.clone(), loading: true, ..Default::default() };
        self.list_scheduled(ctx, e);
    }

    /// Ask for the scheduled posts (SOC-064), in a background job.
    pub fn list_scheduled(&mut self, ctx: &Context, e: &SocialAccountEntry) {
        self.scheduled.loading = true;
        self.scheduled.error = None;
        let (client, src, account) = (self.client_source(e), self.source.clone(), e.account.id.clone());
        let account2 = account.clone();
        run(
            ctx,
            &self.tx,
            move || {
                let (client, result) = match src {
                    Some(s) => (None, s.scheduled()),
                    None => {
                        let mut c = client();
                        let r = c.scheduled();
                        (Some(c), r)
                    }
                };
                Msg::ScheduledList { account, client, result }
            },
            move |m| Msg::ScheduledList { account: account2, client: None, result: Err(m) },
        );
    }

    /// Cancel a scheduled post (SOC-064): it is gone from the list when the server has deleted it.
    pub fn cancel_scheduled(&mut self, ctx: &Context, e: &SocialAccountEntry, id: &str) {
        let key = busy_key_of("cancel", &e.account.id, id);
        if !self.writes.busy.insert(key) {
            return;
        }
        let (client, src, account, id) = (self.client_source(e), self.source.clone(), e.account.id.clone(), id.to_string());
        let (account2, id2) = (account.clone(), id.clone());
        run(
            ctx,
            &self.tx,
            move || {
                let (client, result) = match src {
                    Some(s) => (None, s.cancel_scheduled(&id)),
                    None => {
                        let mut c = client();
                        let r = c.cancel_scheduled(&id);
                        (Some(c), r)
                    }
                };
                Msg::ScheduledCancelled { account, id, client, result }
            },
            move |m| Msg::ScheduledCancelled { account: account2, id: id2, client: None, result: Err(ActionError::Failed(m)) },
        );
    }

    pub(super) fn scheduled_arrived(&mut self, account: &str, result: Result<Vec<Scheduled>, String>) {
        if self.scheduled.account != account {
            return;
        }
        self.scheduled.loading = false;
        match result {
            Ok(mut v) => {
                v.sort_by_key(|s| s.at);
                self.scheduled.items = v;
            }
            Err(e) => self.scheduled.error = Some(e),
        }
    }

    pub(super) fn scheduled_cancelled(&mut self, account: &str, id: &str, result: Result<(), ActionError>) {
        self.writes.busy.remove(&busy_key_of("cancel", account, id));
        match result {
            Ok(()) => {
                if self.scheduled.account == account {
                    self.scheduled.items.retain(|s| s.id != id);
                }
                self.messages.push("Scheduled post cancelled".into());
            }
            Err(ActionError::NeedsWrite) => self.need_write(account),
            Err(ActionError::Failed(m)) => self.messages.push(format!("Could not cancel the scheduled post: {m}")),
        }
    }
}

/// The window with the scheduled posts: when each goes out, what it says, Cancel, Refresh and Close.
pub fn show_list(ctx: &egui::Context, tab_rect: Rect, st: &mut SocialState, entry: &SocialAccountEntry, tab: &TabCtx) {
    if st.scheduled.tab != Some(tab.id) || st.scheduled.account != entry.account.id {
        return;
    }
    let pal = crate::theme::palette();
    let width = (tab_rect.width() - 40.0).clamp(280.0, 520.0);
    let (mut close, mut refresh, mut cancel) = (false, false, None);
    let mut rects: Vec<(String, Rect)> = vec![];
    let busy: Vec<bool> = st.scheduled.items.iter().map(|s| st.writes.busy.contains(&busy_key_of("cancel", &entry.account.id, &s.id))).collect();
    egui::Window::new("Scheduled posts").id(egui::Id::unique(("social-scheduled", tab.id))).collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).fixed_pos(tab_rect.center()).fixed_size(Vec2::new(width, 0.0)).show(
        ctx,
        |ui| {
            ui.set_width(width);
            ui.label(RichText::new(format!("Posts of @{} that the server sends later", entry.account.handle)).size(crate::fonts::px(12.0)).color(pal.muted));
            ui.add_space(4.0);
            if st.scheduled.loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Loading…");
                });
            }
            if let Some(e) = &st.scheduled.error {
                let r = ui.add(egui::Label::new(RichText::new(format!("Could not load the scheduled posts: {e}")).color(pal.link_missing)).wrap());
                rects.push(("scheduled:error".into(), r.rect));
            }
            if st.scheduled.items.is_empty() && !st.scheduled.loading && st.scheduled.error.is_none() {
                let r = ui.label(RichText::new("No scheduled posts.").color(pal.muted));
                rects.push(("scheduled:empty".into(), r.rect));
            }
            for (i, s) in st.scheduled.items.iter().enumerate() {
                super::feed_ui::inset(ui, |ui| {
                    ui.label(RichText::new(s.when()).strong().size(crate::fonts::px(13.0)).color(pal.text_strong));
                    let mut text = short(&super::markup::plain_text(&s.text), 160);
                    if text.is_empty() {
                        text = "(no text)".into();
                    }
                    ui.add(egui::Label::new(RichText::new(text).size(crate::fonts::px(13.0)).color(pal.text)).wrap());
                    let mut facts = vec![];
                    if s.media > 0 {
                        facts.push(format!("{} attachment{}", s.media, if s.media == 1 { "" } else { "s" }));
                    }
                    if s.poll {
                        facts.push("poll".into());
                    }
                    if !s.visibility.is_empty() {
                        facts.push(super::actions::Visibility::from_key(&s.visibility).map_or(s.visibility.clone(), |v| v.label().to_string()));
                    }
                    ui.label(RichText::new(facts.join(" · ")).size(crate::fonts::px(11.5)).color(pal.faint));
                    let (b, spinner) = super::feed_ui::spinner_button(ui, "Cancel post", None, true, busy[i]);
                    if let Some(at) = spinner {
                        rects.push((format!("scheduled:cancel-spinner:{i}"), at));
                    }
                    rects.push((format!("scheduled:cancel:{i}"), b.rect));
                    if b.on_hover_text("Delete this scheduled post: it will not be sent").clicked() {
                        cancel = Some(s.id.clone());
                    }
                });
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                let r = ui.button("Refresh");
                rects.push(("scheduled:refresh".into(), r.rect));
                refresh = r.clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let r = ui.button("Close");
                    rects.push(("scheduled:close".into(), r.rect));
                    close = r.clicked();
                });
            });
        },
    );
    st.rects.extend(rects);
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        close = true;
    }
    if let Some(id) = cancel {
        st.cancel_scheduled(ctx, entry, &id);
    }
    if refresh {
        st.list_scheduled(ctx, entry);
    }
    if close {
        st.scheduled = ScheduledState::default();
    }
}
