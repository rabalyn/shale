//! Mastodon polls (SOC-061, SOC-062): the poll of a status as the feed draws it (options with bars
//! and percentages, a vote control), voting in a background job, and the poll a new post carries
//! (the compose box part is drawn by `compose_extras_ui.rs`). Bluesky has no polls.

use super::actions::{busy_key_of, ActionError};
use super::feed_ui::{spinner_button, Cx};
use super::state::{run, Msg, SocialState};
use super::*;
use crate::theme;
use eframe::egui::{self, Context, RichText, Sense, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One answer of a poll.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PollOption {
    pub title: String,
    /// None while the author hides the totals
    pub votes: Option<u32>,
}

/// The poll of a status.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Poll {
    pub id: String,
    pub options: Vec<PollOption>,
    /// seconds since 1970; None: no end
    pub expires_at: Option<i64>,
    pub expired: bool,
    pub multiple: bool,
    pub votes_count: u32,
    pub voters_count: Option<u32>,
    /// the signed-in account has voted
    pub voted: bool,
    /// the indexes of the options it chose
    pub own_votes: Vec<usize>,
}

impl Poll {
    /// A poll from the JSON of a Mastodon `Poll` (None when it has no id or fewer than two options).
    pub fn parse(v: &Value) -> Option<Poll> {
        let id = text(&v["id"]);
        let options: Vec<PollOption> = v["options"].as_array()?.iter().map(|o| PollOption { title: text(&o["title"]), votes: o["votes_count"].as_u64().map(|_| count(&o["votes_count"])) }).collect();
        if id.is_empty() || options.len() < 2 {
            return None;
        }
        Some(Poll {
            id,
            options,
            expires_at: parse_rfc3339(&text(&v["expires_at"])),
            expired: v["expired"].as_bool().unwrap_or(false),
            multiple: v["multiple"].as_bool().unwrap_or(false),
            votes_count: count(&v["votes_count"]),
            voters_count: v["voters_count"].as_u64().map(|_| count(&v["voters_count"])),
            voted: v["voted"].as_bool().unwrap_or(false),
            own_votes: v["own_votes"].as_array().into_iter().flatten().filter_map(|n| n.as_u64().map(|n| n as usize)).collect(),
        })
    }

    /// The bars and percentages are shown: after voting or when the poll is over, and for the
    /// poll of the account itself (`own`).
    pub fn shows_results(&self, own: bool) -> bool {
        self.voted || self.expired || own
    }

    /// The share of an option in percent: of the people who voted (a poll with several choices
    /// counts people, not votes); None while the totals are hidden.
    pub fn percent(&self, i: usize) -> Option<u32> {
        let votes = self.options.get(i)?.votes?;
        let total = if self.multiple { self.voters_count.unwrap_or(self.votes_count) } else { self.votes_count };
        Some(if total == 0 { 0 } else { ((u64::from(votes) * 100 + u64::from(total) / 2) / u64::from(total)).min(100) as u32 })
    }

    /// "closed", "2 h left" …
    pub fn time_left(&self, now: i64) -> String {
        match self.expires_at {
            _ if self.expired => "closed".into(),
            None => "no end".into(),
            Some(at) if at <= now => "closed".into(),
            Some(at) => format!("{} left", duration_words(at - now)),
        }
    }

    /// Whether the account may vote now.
    pub fn open_for(&self, own: bool) -> bool {
        !self.voted && !self.expired && !own
    }
}

/// "5 min", "3 h", "2 days": the biggest unit that is at least one.
pub fn duration_words(secs: i64) -> String {
    let secs = secs.max(0);
    let (n, unit) = match secs {
        0..=59 => (secs.max(1), "s"),
        60..=3599 => (secs / 60, "min"),
        3600..=86_399 => (secs / 3600, "h"),
        _ => (secs / 86_400, "day"),
    };
    format!("{n} {unit}{}", if unit == "day" && n != 1 { "s" } else { "" })
}

/// The poll of a draft, as it is sent.
#[derive(Clone, Debug, PartialEq)]
pub struct DraftPoll {
    pub options: Vec<String>,
    /// seconds the poll stays open
    pub expires_in: u32,
    pub multiple: bool,
    pub hide_totals: bool,
}

/// What a server allows in a poll (`configuration.polls` of `/api/v2/instance`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PollLimits {
    pub max_options: usize,
    pub max_chars: usize,
    pub min_expiry: u32,
    pub max_expiry: u32,
}

impl Default for PollLimits {
    /// What a server that names nothing gets (SOC-061).
    fn default() -> Self {
        PollLimits { max_options: 4, max_chars: 50, min_expiry: 300, max_expiry: 2_592_000 }
    }
}

impl PollLimits {
    pub fn from_instance(v: &Value) -> PollLimits {
        let d = PollLimits::default();
        let c = &v["configuration"]["polls"];
        let n = |k: &str, d: u64| c[k].as_u64().filter(|n| *n > 0).unwrap_or(d);
        PollLimits {
            max_options: n("max_options", d.max_options as u64).clamp(2, 20) as usize,
            max_chars: n("max_characters_per_option", d.max_chars as u64) as usize,
            min_expiry: n("min_expiration", u64::from(d.min_expiry)).min(u64::from(u32::MAX)) as u32,
            max_expiry: n("max_expiration", u64::from(d.max_expiry)).min(u64::from(u32::MAX)) as u32,
        }
    }
}

/// The durations the picker offers, in seconds, with their words.
const DURATIONS: [(u32, &str); 8] = [(300, "5 minutes"), (1800, "30 minutes"), (3600, "1 hour"), (21_600, "6 hours"), (86_400, "1 day"), (259_200, "3 days"), (604_800, "7 days"), (2_592_000, "30 days")];

/// The poll being written in the compose box.
#[derive(Clone, Debug, PartialEq)]
pub struct ComposePoll {
    pub options: Vec<String>,
    pub expires_in: u32,
    pub multiple: bool,
    pub hide_totals: bool,
}

impl Default for ComposePoll {
    fn default() -> Self {
        ComposePoll { options: vec![String::new(), String::new()], expires_in: 86_400, multiple: false, hide_totals: false }
    }
}

impl ComposePoll {
    /// The durations the server allows, from the list of the picker (the nearest allowed one is added when the list has none).
    pub fn durations(limits: &PollLimits) -> Vec<(u32, String)> {
        let mut v: Vec<(u32, String)> = DURATIONS.iter().filter(|(s, _)| (limits.min_expiry..=limits.max_expiry).contains(s)).map(|(s, w)| (*s, (*w).to_string())).collect();
        if v.is_empty() {
            v.push((limits.min_expiry, duration_words(i64::from(limits.min_expiry))));
        }
        v
    }

    /// Keep the chosen duration inside what the server allows.
    pub fn fit(&mut self, limits: &PollLimits) {
        self.options.truncate(limits.max_options);
        let all = ComposePoll::durations(limits);
        if !all.iter().any(|(s, _)| *s == self.expires_in) {
            self.expires_in = all.iter().map(|(s, _)| *s).min_by_key(|s| s.abs_diff(self.expires_in)).unwrap_or(limits.min_expiry);
        }
    }

    /// Why the poll cannot be sent; None when it can.
    pub fn problem(&self, limits: &PollLimits) -> Option<String> {
        let filled: Vec<&str> = self.options.iter().map(|o| o.trim()).filter(|o| !o.is_empty()).collect();
        if filled.len() < 2 {
            return Some("A poll needs at least two options".into());
        }
        if filled.len() > limits.max_options {
            return Some(format!("A poll has at most {} options", limits.max_options));
        }
        if let Some(o) = filled.iter().find(|o| o.chars().count() > limits.max_chars) {
            return Some(format!("The option “{}” is longer than {} characters", super::feed_ui::short(o, 20), limits.max_chars));
        }
        let mut seen = HashSet::new();
        if filled.iter().any(|o| !seen.insert(o.to_lowercase())) {
            return Some("Two options of the poll are the same".into());
        }
        None
    }

    /// The poll as it is sent: the empty options are left out.
    pub fn draft(&self) -> DraftPoll {
        DraftPoll { options: self.options.iter().map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect(), expires_in: self.expires_in, multiple: self.multiple, hide_totals: self.hide_totals }
    }
}

/// Which options of the polls on screen are ticked (by `post_key`), before the vote is sent.
pub type Selected = HashMap<String, Vec<bool>>;

/// Draw the poll of a post: the options with bars and percentages, or radio buttons / check
/// boxes and a Vote button. True when Vote was pressed.
pub(super) fn card(ui: &mut egui::Ui, cx: &mut Cx, p: &Post, poll: &Poll, idx: usize) -> bool {
    let pal = theme::palette();
    let own = cx.account.split_once(':').is_some_and(|(_, h)| h.eq_ignore_ascii_case(&p.author.handle));
    let key = super::state::post_key(p);
    let results = poll.shows_results(own);
    let mut sel = cx.polls.get(&key).cloned().unwrap_or_default();
    sel.resize(poll.options.len(), false);
    let mut vote = false;
    for (i, o) in poll.options.iter().enumerate() {
        if results {
            let width = ui.available_width();
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 24.0), Sense::hover());
            let share = poll.percent(i);
            let frac = share.map_or(0.0, |s| s as f32 / 100.0);
            ui.painter().rect_filled(rect, 4.0, pal.bg_active);
            if frac > 0.0 {
                let mut bar = rect;
                bar.set_width(rect.width() * frac);
                ui.painter().rect_filled(bar, 4.0, pal.accent.gamma_multiply(0.45));
            }
            let mine = poll.own_votes.contains(&i);
            let label = format!("{}{}", if mine { "✓ " } else { "" }, super::feed_ui::short(&o.title, 80));
            ui.painter().text(rect.left_center() + Vec2::new(6.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(crate::fonts::px(13.5)), pal.text);
            let pct = share.map_or("–".to_string(), |s| format!("{s}%"));
            ui.painter().text(rect.right_center() - Vec2::new(6.0, 0.0), egui::Align2::RIGHT_CENTER, pct, egui::FontId::proportional(crate::fonts::px(13.0)), pal.text_strong);
            cx.rects.push((format!("poll:{idx}:option:{i}"), resp.rect));
        } else if poll.multiple {
            let r = crate::clickable::pointer(ui.checkbox(&mut sel[i], &o.title));
            cx.rects.push((format!("poll:{idx}:option:{i}"), r.rect));
        } else {
            let r = crate::clickable::pointer(ui.radio(sel[i], &o.title));
            if r.clicked() {
                sel.iter_mut().for_each(|s| *s = false);
                sel[i] = true;
            }
            cx.rects.push((format!("poll:{idx}:option:{i}"), r.rect));
        }
    }
    let busy = cx.busy.contains(&busy_key_of("vote", cx.account, &p.id));
    let open = poll.open_for(own);
    ui.horizontal(|ui| {
        if open {
            let enabled = sel.iter().any(|s| *s);
            let (b, spinner) = spinner_button(ui, "Vote", Some(pal.accent.gamma_multiply(0.55)), enabled, busy);
            if let Some(at) = spinner {
                cx.rects.push((format!("poll:{idx}:vote-spinner"), at));
            }
            let hint = if poll.multiple { "Tick one or more options, then vote" } else { "Choose an option, then vote" };
            vote = cx.rec(format!("poll:{idx}:vote"), b).on_hover_text(hint).clicked();
        }
        let votes = match poll.voters_count.filter(|_| poll.multiple) {
            Some(n) => format!("{n} {}", if n == 1 { "voter" } else { "voters" }),
            None => format!("{} {}", poll.votes_count, if poll.votes_count == 1 { "vote" } else { "votes" }),
        };
        ui.label(RichText::new(format!("{votes} · {}", poll.time_left(cx.now))).size(crate::fonts::px(12.0)).color(pal.muted));
    });
    cx.polls.insert(key, sel);
    vote
}

impl SocialState {
    /// Send the choices ticked on the poll of `post` (SOC-062) in a background job; the button
    /// shows its spinner until the server has answered. A post without a poll or without a choice
    /// does nothing.
    pub fn vote(&mut self, ctx: &Context, e: &SocialAccountEntry, post: &Post) {
        let Some(poll) = post.poll.clone() else { return };
        let key = busy_key_of("vote", &e.account.id, &post.id);
        let choices: Vec<usize> = self.polls.get(&super::state::post_key(post)).map(|s| s.iter().enumerate().filter(|(_, on)| **on).map(|(i, _)| i).collect()).unwrap_or_default();
        if choices.is_empty() || self.writes.busy.contains(&key) {
            return;
        }
        if e.account.may_write() == Some(false) {
            self.need_write(&e.account.id);
            return;
        }
        self.writes.busy.insert(key);
        let (client, src, account, post_id) = (self.client_source(e), self.source.clone(), e.account.id.clone(), post.id.clone());
        let (account2, post2) = (account.clone(), post_id.clone());
        run(
            ctx,
            &self.tx,
            move || {
                let (client, result) = match src {
                    Some(s) => (None, s.vote(&poll, &choices)),
                    None => {
                        let mut c = client();
                        let r = c.vote(&poll, &choices);
                        (Some(c), r)
                    }
                };
                Msg::Voted { account, post: post_id, client, result }
            },
            move |m| Msg::Voted { account: account2, post: post2, client: None, result: Err(ActionError::Failed(m)) },
        );
    }

    /// A vote ended: the poll of the post changes where it is held (the server's answer is the
    /// new poll), or a message says why not.
    pub(super) fn voted(&mut self, account: &str, post: &str, result: Result<Poll, ActionError>) {
        self.writes.busy.remove(&busy_key_of("vote", account, post));
        match result {
            Ok(poll) => {
                self.polls.retain(|k, _| !k.starts_with(&format!("{post}|")));
                let apply = |p: &mut Post| p.poll = Some(poll.clone());
                self.update_post(account, post, apply);
                self.index.update_post(account, post, apply);
            }
            Err(ActionError::NeedsWrite) => self.need_write(account),
            Err(ActionError::Failed(m)) => self.messages.push(format!("Could not vote: {m}")),
        }
    }
}
