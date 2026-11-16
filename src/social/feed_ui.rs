//! The feed view: pick an account and a feed, scroll the posts, save one to the vault.
//! A centred column like the task list; the next page loads when the end of the list comes
//! into view; pictures come from `ImageCache` (never from the UI thread's network).

use super::images::ImageCache;
use super::state::{post_key, FeedsState, SocialState};
use super::{FeedRef, MediaKind, Network, Post, SocialAccountEntry};
use crate::media;
use crate::theme;
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{self, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, Ui, Vec2};
use std::collections::HashSet;

/// What the view asks the app to do.
#[derive(Debug, PartialEq)]
pub enum Ask {
    /// open the "Feed accounts" dialog
    Accounts,
    /// the user chose a feed: show it in this tab, or in a new one
    Pick { feed: FeedRef, new_tab: bool },
    /// the tab named no feed (or one that is gone): this is the one shown; no history step
    Resolve(FeedRef),
}

/// What the view needs to know about the tab it is shown in.
pub struct TabCtx {
    /// the tab's number: its scroll position belongs to it
    pub id: u64,
    pub feed: Option<FeedRef>,
    /// the account and feed shown last (from the settings)
    pub last: Option<(String, String)>,
    /// plain keys (the `.` key) may be used: no dialog is open
    pub keys: bool,
}

/// Where a tab's view is held still while posts come in above it (SOC-028): the post at the top
/// of the window and where its top edge is in the window. When the posts above it change in
/// height (new posts put in, a picture arrived) the scroll offset is set so that it stays there;
/// when the user scrolls, the anchor moves with the view.
#[derive(Clone, Debug, Default)]
pub struct Anchor {
    /// the feed this belongs to
    pub scope: (String, String),
    /// `Timeline::reloads` last seen: a list that was replaced starts at the top
    pub reloads: u32,
    pub key: String,
    pub dy: f32,
    /// the scroll offset to set before the next layout
    pub apply: Option<f32>,
}

/// Open a web address in the browser, only if it is http(s) (the address came from a server).
pub fn open_web(url: &str) {
    let l = url.trim().to_ascii_lowercase();
    if l.starts_with("https://") || l.starts_with("http://") {
        if let Err(e) = crate::opener::open_url(url.trim()) {
            eprintln!("could not open {url}: {e}");
        }
    }
}

/// The height of the row above the list that holds the button for new posts.
const NEW_ROW_HEIGHT: f32 = 28.0;
/// Side of the spinner inside a busy button.
const BUTTON_SPINNER: f32 = 12.0;

/// A button that shows a running job as a spinner inside itself (SOC-041, SOC-042). It keeps
/// the room for the spinner also when idle, so it has one size and nothing next to it moves;
/// while `busy` it is disabled. Returns the button and, while busy, where the spinner is.
fn spinner_button(ui: &mut Ui, text: impl Into<egui::WidgetText>, fill: Option<Color32>, enabled: bool, busy: bool) -> (egui::Response, Option<Rect>) {
    let text: egui::WidgetText = text.into();
    let pad = ui.spacing().button_padding.x;
    let label_w = text.clone().into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button).size().x;
    let mut button = egui::Button::new(text).min_size(egui::vec2(label_w + 2.0 * (BUTTON_SPINNER + pad) + 2.0 * pad, 0.0));
    if let Some(fill) = fill {
        button = button.fill(fill);
    }
    let r = ui.add_enabled(enabled && !busy, button);
    let spinner = busy.then(|| {
        let at = Rect::from_center_size(egui::pos2(r.rect.left() + pad + BUTTON_SPINNER / 2.0, r.rect.center().y), egui::Vec2::splat(BUTTON_SPINNER));
        egui::Spinner::new().size(BUTTON_SPINNER).paint_at(ui, at);
        at
    });
    (r, spinner)
}

/// "just now", "2 min ago", "3 h ago".
fn checked_ago(secs: u64) -> String {
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        _ => format!("{} h ago", secs / 3600),
    }
}

/// "now", "5m", "3h", "2d", or the date.
pub fn age_text(at: i64, now: i64) -> String {
    let s = now - at;
    match s {
        _ if at == 0 => String::new(),
        ..60 => "now".into(),
        60..3600 => format!("{}m", s / 60),
        3600..86_400 => format!("{}h", s / 3600),
        86_400..2_592_000 => format!("{}d", s / 86_400),
        _ => crate::import::local_stamp(at, true),
    }
}

/// The text of a post as pieces: plain text and `[label](address)` links (also bare addresses).
pub fn segments(md: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = vec![];
    let mut plain = String::new();
    let flush = |plain: &mut String, out: &mut Vec<(String, Option<String>)>| {
        if plain.is_empty() {
            return;
        }
        // bare addresses inside the plain text become links too
        let mut word = String::new();
        let mut text = String::new();
        let push_word = |word: &mut String, text: &mut String, out: &mut Vec<(String, Option<String>)>| {
            if word.starts_with("https://") || word.starts_with("http://") {
                if !text.is_empty() {
                    out.push((std::mem::take(text), None));
                }
                let trimmed = word.trim_end_matches(['.', ',', ')', ';', '!', '?']).to_string();
                let tail = word[trimmed.len()..].to_string();
                out.push((trimmed.clone(), Some(trimmed)));
                text.push_str(&tail);
            } else {
                text.push_str(word);
            }
            word.clear();
        };
        for c in plain.chars() {
            if c.is_whitespace() {
                push_word(&mut word, &mut text, out);
                text.push(c);
            } else {
                word.push(c);
            }
        }
        push_word(&mut word, &mut text, out);
        if !text.is_empty() {
            out.push((text, None));
        }
        plain.clear();
    };
    let mut rest = md;
    while let Some(p) = rest.find('[') {
        plain.push_str(&rest[..p]);
        let after = &rest[p + 1..];
        // the first unescaped "](" of this line
        let mut close = None;
        let b = after.as_bytes();
        let mut i = 0;
        while i + 1 < b.len() && b[i] != b'\n' {
            if b[i] == b'\\' {
                i += 2;
                continue;
            }
            if b[i] == b']' && b[i + 1] == b'(' {
                close = Some(i);
                break;
            }
            i += 1;
        }
        if let Some(close) = close {
            if let Some(end) = after[close + 2..].find(')') {
                flush(&mut plain, &mut out);
                let label = after[..close].replace("\\[", "[").replace("\\]", "]");
                out.push((label, Some(after[close + 2..close + 2 + end].to_string())));
                rest = &after[close + 2 + end + 1..];
                continue;
            }
        }
        plain.push('[');
        rest = after;
    }
    plain.push_str(rest);
    flush(&mut plain, &mut out);
    out
}

/// A wrapped paragraph with clickable links. Returns the address that was clicked.
fn rich_text(ui: &mut Ui, md: &str, size: f32, color: Color32) -> Option<String> {
    let parts = segments(md);
    let mut job = LayoutJob::default();
    job.wrap.max_width = ui.available_width();
    let mut ranges: Vec<(usize, usize, String)> = vec![];
    let mut at = 0;
    for (text, link) in &parts {
        let n = text.chars().count();
        let fmt = match link {
            Some(_) => TextFormat { font_id: FontId::proportional(size), color: theme::LINK, underline: Stroke::new(1.0, theme::LINK.gamma_multiply(0.6)), ..Default::default() },
            None => TextFormat { font_id: FontId::proportional(size), color, ..Default::default() },
        };
        job.append(text, 0.0, fmt);
        if let Some(u) = link {
            ranges.push((at, at + n, u.clone()));
        }
        at += n;
    }
    let (pos, galley, resp) = egui::Label::new(job).sense(Sense::click()).layout_in_ui(ui);
    ui.painter().galley(pos, galley.clone(), color);
    let hit = |p: egui::Pos2| {
        let i = galley.cursor_from_pos(p - pos).index.0;
        ranges.iter().find(|(a, b, _)| i >= *a && i < *b).map(|r| r.2.clone())
    };
    if let Some(p) = resp.hover_pos() {
        if hit(p).is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    if resp.clicked() {
        return resp.interact_pointer_pos().and_then(hit);
    }
    None
}

/// What is needed to draw a post.
struct Cx<'a> {
    images: &'a ImageCache,
    revealed: &'a mut HashSet<String>,
    saving: &'a HashSet<String>,
    rects: &'a mut Vec<(String, Rect)>,
    now: i64,
}

impl Cx<'_> {
    fn rec(&mut self, key: String, r: egui::Response) -> egui::Response {
        self.rects.push((key, r.rect));
        r
    }
}

fn avatar(ui: &mut Ui, cx: &Cx, url: &str, name: &str, size: f32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let tex = cx.images.get(url).and_then(|p| media::texture(ui.ctx(), &p));
    match tex {
        Some(t) => {
            egui::Image::from_texture(egui::load::SizedTexture::from_handle(&t)).fit_to_exact_size(Vec2::splat(size)).corner_radius(CornerRadius::same((size / 2.0) as u8)).paint_at(ui, r);
        }
        None => {
            ui.painter().circle_filled(r.center(), size / 2.0, theme::BG_ACTIVE);
            let initial = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_default();
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, initial, FontId::proportional(size * 0.45), theme::MUTED);
        }
    }
}

/// A picture (or the preview of a video) in a cell; None while it is still coming.
fn thumb(ui: &mut Ui, cx: &mut Cx, key: String, m: &super::Media, cell: Vec2, natural: bool) {
    let url = if m.preview_url.is_empty() && m.kind == MediaKind::Image { &m.url } else { &m.preview_url };
    let tex = cx.images.get(url).and_then(|p| media::texture(ui.ctx(), &p));
    // a single picture keeps its shape; several fill equal cells
    let size = match (&tex, natural) {
        (Some(t), true) => {
            let s = t.size_vec2();
            let k = (cell.x / s.x).min(cell.y / s.y).min(1.5);
            s * k
        }
        _ => cell,
    };
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let radius = CornerRadius::same(6);
    ui.painter().rect_filled(r, radius, theme::BG_PRIMARY);
    match &tex {
        Some(t) => {
            egui::Image::from_texture(egui::load::SizedTexture::from_handle(t)).fit_to_exact_size(size).corner_radius(radius).paint_at(ui, r);
        }
        None => {
            let word = if cx.images.failed(url) {
                "no preview"
            } else if url.is_empty() {
                ""
            } else {
                "loading…"
            };
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, word, FontId::proportional(12.0), theme::FAINT);
        }
    }
    if matches!(m.kind, MediaKind::Video | MediaKind::Gifv) {
        // a play mark: a dark disc with a triangle
        let c = r.center();
        ui.painter().circle_filled(c, 22.0, Color32::from_black_alpha(150));
        ui.painter().add(egui::Shape::convex_polygon(vec![c + Vec2::new(-6.0, -10.0), c + Vec2::new(-6.0, 10.0), c + Vec2::new(11.0, 0.0)], Color32::WHITE, Stroke::NONE));
        let tag = if m.kind == MediaKind::Gifv { "GIF" } else { "VIDEO" };
        let g = ui.painter().layout_no_wrap(tag.into(), FontId::proportional(10.5), Color32::WHITE);
        let box_ = Rect::from_min_size(r.left_bottom() + Vec2::new(6.0, -6.0 - g.size().y - 4.0), g.size() + Vec2::new(8.0, 4.0));
        ui.painter().rect_filled(box_, CornerRadius::same(3), Color32::from_black_alpha(160));
        ui.painter().galley(box_.min + Vec2::new(4.0, 2.0), g, Color32::WHITE);
    }
    if !m.alt.trim().is_empty() {
        let g = ui.painter().layout_no_wrap("ALT".into(), FontId::proportional(10.0), Color32::WHITE);
        let box_ = Rect::from_min_size(r.right_bottom() - Vec2::new(g.size().x + 14.0, g.size().y + 10.0), g.size() + Vec2::new(8.0, 4.0));
        ui.painter().rect_filled(box_, CornerRadius::same(3), Color32::from_black_alpha(160));
        ui.painter().galley(box_.min + Vec2::new(4.0, 2.0), g, Color32::WHITE);
    }
    let resp = cx.rec(key, resp);
    let resp = if m.alt.trim().is_empty() { resp } else { resp.on_hover_text(m.alt.trim()) };
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        open_web(&m.url);
    }
}

fn media_block(ui: &mut Ui, cx: &mut Cx, p: &Post, idx: usize, width: f32) {
    let shown: Vec<&super::Media> = p.media.iter().collect();
    let visual: Vec<(usize, &super::Media)> = shown.iter().copied().enumerate().filter(|(_, m)| matches!(m.kind, MediaKind::Image | MediaKind::Video | MediaKind::Gifv)).collect();
    if visual.len() == 1 {
        let (k, m) = visual[0];
        thumb(ui, cx, format!("media:{idx}:{k}"), m, Vec2::new(width, 340.0), true);
    } else if !visual.is_empty() {
        let cell = Vec2::new((width - 6.0) / 2.0, 170.0);
        egui::Grid::new(("social-media", idx)).spacing([6.0, 6.0]).show(ui, |ui| {
            for (n, (k, m)) in visual.iter().enumerate() {
                thumb(ui, cx, format!("media:{idx}:{k}"), m, cell, false);
                if n % 2 == 1 {
                    ui.end_row();
                }
            }
        });
    }
    for (k, m) in shown.iter().enumerate().filter(|(_, m)| !matches!(m.kind, MediaKind::Image | MediaKind::Video | MediaKind::Gifv)) {
        let name = if m.file_name_hint.is_empty() { "file" } else { m.file_name_hint.as_str() };
        let label = format!("{} · {name}", if m.kind == MediaKind::Audio { "Audio" } else { "File" });
        if cx.rec(format!("media:{idx}:{k}"), ui.link(label)).clicked() {
            open_web(&m.url);
        }
    }
}

/// A box with a lighter fill inside a post (quote, link card).
fn inset(ui: &mut Ui, f: impl FnOnce(&mut Ui)) -> egui::Response {
    egui::Frame::new()
        .fill(theme::BG_PRIMARY)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(6)
        .inner_margin(8)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            f(ui)
        })
        .response
}

fn short(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > n {
        format!("{}…", one.chars().take(n).collect::<String>().trim_end())
    } else {
        one
    }
}

/// One post. Returns true when "Save to vault" was clicked, and where the card is.
fn post_card(ui: &mut Ui, cx: &mut Cx, p: &Post, idx: usize, width: f32) -> (bool, Rect) {
    let key = post_key(p);
    let mut save = false;
    let inner = width - 24.0;
    let card = egui::Frame::new().fill(theme::BG_SECONDARY).corner_radius(8).inner_margin(12).show(ui, |ui| {
        ui.set_width(inner);
        if let Some(b) = &p.reposted_by {
            let word = if p.network == Network::Mastodon { "boosted" } else { "reposted" };
            ui.label(RichText::new(format!("🔁 {} {word}", b.name())).size(12.0).color(theme::MUTED));
            ui.add_space(2.0);
        }
        ui.horizontal(|ui| {
            avatar(ui, cx, &p.author.avatar_url, p.author.name(), 38.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.horizontal(|ui| {
                    if ui.add(egui::Label::new(RichText::new(p.author.name()).strong().size(14.5).color(theme::TEXT_STRONG)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Open the profile").clicked()
                    {
                        open_web(&p.author.profile_url);
                    }
                    let age = age_text(p.created_at, cx.now);
                    if !age.is_empty() {
                        ui.label(RichText::new(format!("· {age}")).size(12.5).color(theme::MUTED)).on_hover_text(crate::import::local_stamp(p.created_at, false));
                    }
                    if p.reply_to.is_some() {
                        ui.label(RichText::new("· reply").size(12.5).color(theme::FAINT));
                    }
                });
                ui.label(RichText::new(format!("@{}", p.author.handle)).size(12.5).color(theme::MUTED));
            });
        });
        ui.add_space(6.0);
        let mut open_body = true;
        if let Some(cw) = &p.content_warning {
            let shown = cx.revealed.contains(&key);
            open_body = shown;
            inset(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(format!("Content warning: {cw}")).size(13.5).color(theme::TEXT)).wrap());
                });
                if cx.rec(format!("cw:{idx}"), ui.small_button(if shown { "Hide" } else { "Show" })).clicked() {
                    if shown {
                        cx.revealed.remove(&key);
                    } else {
                        cx.revealed.insert(key.clone());
                    }
                }
            });
            ui.add_space(4.0);
        }
        if open_body {
            for para in p.text_markdown.split("\n\n").filter(|t| !t.trim().is_empty()) {
                if let Some(url) = rich_text(ui, para.trim(), 14.5, theme::TEXT) {
                    open_web(&url);
                }
                ui.add_space(4.0);
            }
            if !p.media.is_empty() {
                media_block(ui, cx, p, idx, inner);
                ui.add_space(4.0);
            }
            if let Some(q) = &p.quote {
                inset(ui, |ui| {
                    ui.label(RichText::new(format!("{} @{}", q.author.name(), q.author.handle)).size(12.5).color(theme::MUTED));
                    let text = super::markup::plain_text(&q.text_markdown);
                    if !text.is_empty() {
                        ui.add(egui::Label::new(RichText::new(short(&text, 280)).size(13.5).color(theme::TEXT)).wrap());
                    }
                    if !q.media.is_empty() {
                        ui.label(RichText::new(format!("{} attachment{}", q.media.len(), if q.media.len() == 1 { "" } else { "s" })).size(12.0).color(theme::FAINT));
                    }
                });
                ui.add_space(4.0);
            }
            if let Some(c) = &p.link_card {
                let r = inset(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(short(if c.title.trim().is_empty() { &c.url } else { &c.title }, 120)).strong().size(13.5).color(theme::TEXT_STRONG)).wrap());
                    if !c.description.trim().is_empty() {
                        ui.add(egui::Label::new(RichText::new(short(&c.description, 200)).size(12.5).color(theme::MUTED)).wrap());
                    }
                    ui.label(RichText::new(crate::calendar::dav::host_of(&c.url)).size(11.5).color(theme::FAINT));
                });
                if r.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(&c.url).clicked() {
                    open_web(&c.url);
                }
                ui.add_space(4.0);
            }
        }
        // counts and buttons
        ui.horizontal(|ui| {
            let busy = cx.saving.contains(&key);
            let fill = theme::ACCENT.gamma_multiply(0.55);
            let (b, spinner) = spinner_button(ui, RichText::new("Save to vault").strong().color(theme::TEXT_STRONG), Some(fill), true, busy);
            if let Some(at) = spinner {
                cx.rects.push((format!("save-spinner:{idx}"), at));
            }
            if cx.rec(format!("save:{idx}"), b).on_hover_text("Copy this post (the whole thread of its author, with pictures and a link back) into your vault").clicked() {
                save = true;
            }
            if cx.rec(format!("open:{idx}"), ui.button("Open in browser")).clicked() {
                open_web(&p.url);
            }
            if cx.rec(format!("copy:{idx}"), ui.button("Copy link")).clicked() {
                ui.ctx().copy_text(p.url.clone());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (a, b) = if p.network == Network::Mastodon { ("boosts", "favourites") } else { ("reposts", "likes") };
                ui.label(RichText::new(format!("{} replies · {} {a} · {} {b}", p.reply_count, p.boost_count, p.like_count)).size(11.5).color(theme::FAINT));
            });
        });
    });
    (save, card.response.rect)
}

/// A column centred in the window.
fn column(ui: &mut Ui, width: f32, f: impl FnOnce(&mut Ui)) {
    let full = ui.available_width();
    let pad = ((full - width) / 2.0).max(0.0);
    ui.horizontal_top(|ui| {
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_width(width);
            f(ui);
        });
    });
}

fn account_label(e: &SocialAccountEntry) -> String {
    format!("{} · @{}", e.account.network.label(), e.account.handle)
}

/// The whole view of one tab.
pub fn show(ui: &mut Ui, st: &mut SocialState, accounts: &[SocialAccountEntry], tab: &TabCtx) -> Vec<Ask> {
    let mut asks = vec![];
    let ctx = ui.ctx().clone();
    st.rects.clear();
    let full = ui.available_width();
    let width = (full - 32.0).clamp(280.0, 700.0);
    let Some(entry) = SocialState::account_for(accounts, tab.feed.as_ref().map(|f| f.account.as_str()), tab.last.as_ref()).cloned() else {
        // no account yet
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.25);
            ui.label(RichText::new("Read your feeds here").size(24.0).color(theme::TEXT_STRONG));
            ui.add_space(6.0);
            ui.label(RichText::new("Add a Mastodon or Bluesky account, scroll its timeline and\ncopy posts, threads and pictures into your vault as notes.").size(14.0).color(theme::MUTED));
            ui.add_space(14.0);
            let r = ui.add(egui::Button::new(RichText::new("Add account").strong().size(15.0)).fill(theme::ACCENT.gamma_multiply(0.55)).min_size(Vec2::new(150.0, 32.0)));
            st.rects.push(("add-account".into(), r.rect));
            if r.clicked() {
                asks.push(Ask::Accounts);
            }
        });
        return asks;
    };
    let aid = entry.account.id.clone();
    st.ensure_feeds(&ctx, &entry);
    if st.images.is_none() {
        st.images = Some(ImageCache::new(&ctx, st.cache_dir.clone()));
    }
    let want = tab.feed.as_ref().filter(|f| f.account == aid).map(|f| f.feed.as_str()).unwrap_or("");
    let feed = st.resolve_feed(&aid, want, tab.last.as_ref());
    if let Some(f) = &feed {
        let shown = FeedRef { account: aid.clone(), feed: f.id.clone() };
        st.feed_of.insert(aid.clone(), f.id.clone());
        if tab.feed.as_ref() != Some(&shown) {
            asks.push(Ask::Resolve(shown));
        }
        st.ensure_timeline(&ctx, &entry, f);
    }
    // ---- top bar
    ui.add_space(8.0);
    let mut check_now = false;
    let mut reload = false;
    let mut retry_feeds = false;
    let mut pick: Option<(FeedRef, bool)> = None;
    column(ui, width, |ui| {
        ui.horizontal(|ui| {
            let c = egui::ComboBox::from_id_salt(("social-account", tab.id)).selected_text(account_label(&entry)).width(210.0).show_ui(ui, |ui| {
                for e in accounts {
                    if ui.selectable_label(e.account.id == aid, account_label(e)).clicked() {
                        pick = Some((FeedRef { account: e.account.id.clone(), feed: String::new() }, false));
                    }
                }
            });
            st.rects.push(("account-combo".into(), c.response.rect));
            let mut seen_rects: Vec<(String, Rect)> = vec![];
            match st.feeds.get(&aid) {
                Some(FeedsState::Ready(list)) => {
                    let name = feed.as_ref().map(|f| f.title.clone()).unwrap_or_default();
                    let c = egui::ComboBox::from_id_salt(("social-feed", tab.id)).selected_text(name).width(180.0).show_ui(ui, |ui| {
                        for f in list {
                            ui.horizontal(|ui| {
                                let r = ui.selectable_label(feed.as_ref().map(|c| c.id == f.id).unwrap_or(false), &f.title);
                                seen_rects.push((format!("feed:{}", f.id), r.rect));
                                // Shift+click and middle-click open a new tab, as for links in notes
                                let new_tab = r.middle_clicked() || (r.clicked() && ui.input(|i| i.modifiers.shift));
                                if r.clicked() || r.middle_clicked() {
                                    pick = Some((FeedRef { account: aid.clone(), feed: f.id.clone() }, new_tab));
                                }
                                let b = ui.small_button("New tab").on_hover_text("Open this feed in a new tab");
                                seen_rects.push((format!("feed-new-tab:{}", f.id), b.rect));
                                if b.clicked() {
                                    pick = Some((FeedRef { account: aid.clone(), feed: f.id.clone() }, true));
                                }
                            });
                        }
                    });
                    seen_rects.push(("feed-combo".into(), c.response.rect));
                }
                Some(FeedsState::Failed(_)) => {
                    let r = ui.button("Reload feeds");
                    seen_rects.push(("reload-feeds".into(), r.rect));
                    retry_feeds = r.clicked();
                }
                _ => {
                    ui.spinner();
                }
            }
            let checking = feed.as_ref().and_then(|f| st.timelines.get(&(aid.clone(), f.id.clone()))).map(|t| t.checking).unwrap_or(false);
            let (r, spinner) = spinner_button(ui, "Check now", None, feed.is_some(), checking);
            seen_rects.push(("check-now".into(), r.rect));
            if let Some(at) = spinner {
                seen_rects.push(("check-now-spinner".into(), at));
            }
            check_now = r.on_hover_text("Look for new posts now. They wait behind the button at the top of the list.").clicked();
            let menu = ui.menu_button("⋯", |ui| {
                let r = ui.add_enabled(feed.is_some(), egui::Button::new("Reload from the top"));
                seen_rects.push(("reload".into(), r.rect));
                if r.on_hover_text("Load the first page again and replace the list").clicked() {
                    reload = true;
                    ui.close();
                }
            });
            seen_rects.push(("feed-menu".into(), menu.response.rect));
            st.rects.extend(seen_rects);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = ui.button("Accounts…");
                st.rects.push(("accounts".into(), r.rect));
                if r.clicked() {
                    asks.push(Ask::Accounts);
                }
            });
        });
        if let Some(FeedsState::Failed(e)) = st.feeds.get(&aid) {
            ui.add(egui::Label::new(RichText::new(format!("Could not load the feeds: {e}")).color(theme::LINK_MISSING)).wrap());
        }
    });
    if let Some((feed, new_tab)) = pick {
        asks.push(Ask::Pick { feed, new_tab });
    }
    if retry_feeds {
        st.reload_feeds(&ctx, &entry);
    }
    if let Some(f) = &feed {
        if check_now {
            st.check(&ctx, &entry, f);
        }
        if reload {
            st.reload(&ctx, &entry, f);
        }
    }
    ui.add_space(6.0);
    let Some(feed) = feed else { return asks };
    // ---- posts
    // ---- the row for new posts: always there, of one height, so the list never moves when the button comes and goes
    let key = (aid.clone(), feed.id.clone());
    let waiting = st.timelines.get(&key).map(|t| t.waiting.len()).unwrap_or(0);
    let mut show_new = tab.keys && waiting > 0 && !ctx.egui_wants_keyboard_input() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Period));
    {
        let (checking, check_error, last) = st.timelines.get(&key).map(|t| (t.checking, t.check_error.clone(), t.last_checked)).unwrap_or((false, None, None));
        let size = Vec2::new(ui.available_width(), NEW_ROW_HEIGHT);
        ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.set_min_size(size);
            ui.set_max_height(NEW_ROW_HEIGHT);
            if waiting > 0 {
                let label = format!("{waiting} new post{} — show", if waiting == 1 { "" } else { "s" });
                let r = ui.add(egui::Button::new(RichText::new(label).strong().color(theme::TEXT_STRONG)).fill(theme::ACCENT).corner_radius(12).min_size(Vec2::new(0.0, 24.0)));
                st.rects.push(("new-posts".into(), r.rect));
                show_new |= r.on_hover_text("Put the new posts above the list (key: .). The view stays where it is.").clicked();
            } else {
                ui.add_space(5.0);
                if let Some(e) = &check_error {
                    let r = ui.add(egui::Label::new(RichText::new(format!("Could not look for new posts: {e}")).size(11.5).color(theme::LINK_MISSING)).truncate());
                    st.rects.push(("check-error".into(), r.rect));
                } else if checking {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(12.0));
                        ui.label(RichText::new("Checking…").size(11.5).color(theme::MUTED));
                    });
                } else {
                    let text = match last {
                        Some(t) => format!("Up to date · checked {}", checked_ago(t.elapsed().as_secs())),
                        None => "Up to date".to_string(),
                    };
                    let r = ui.label(RichText::new(text).size(11.5).color(theme::MUTED));
                    st.rects.push(("up-to-date".into(), r.rect));
                    ctx.request_repaint_after(std::time::Duration::from_secs(20));
                }
            }
        });
    }
    // a search hit for this tab: its post is put into the list and the view is moved to it (SOC-035)
    let focused = st.take_focus(tab.id, &key);
    let posts = st.timelines.get_mut(&key).map(|t| std::mem::take(&mut t.posts)).unwrap_or_default();
    let (loading, loaded, error, has_more, filling) = st.timelines.get(&key).map(|t| (t.loading, t.loaded, t.error.clone(), t.cursor.is_some(), t.filling)).unwrap_or((false, false, None, false, false));
    let gaps: HashSet<String> = st.timelines.get(&key).map(|t| t.gaps.iter().map(|g| g.above.clone()).collect()).unwrap_or_default();
    let reloads = st.timelines.get(&key).map(|t| t.reloads).unwrap_or(0);
    let mut save_post: Option<Post> = None;
    let mut fill_gap: Option<String> = None;
    let mut retry = false;
    let mut near_end = false;
    let mut anchor = st.anchors.remove(&tab.id).filter(|a| a.scope == key).unwrap_or_else(|| Anchor { scope: key.clone(), reloads, ..Default::default() });
    if anchor.reloads != reloads {
        // the list was replaced: it starts at the top
        anchor = Anchor { scope: key.clone(), reloads, apply: Some(0.0), ..Default::default() };
    }
    if let Some(k) = focused {
        anchor = Anchor { scope: key.clone(), reloads, key: k, dy: 0.0, apply: None };
    }
    let applied = anchor.apply.take();
    let mut post_rects: Vec<(String, Rect)> = vec![];
    let mut content_top = 0.0;
    let out = {
        let SocialState { images, revealed, saving, rects, .. } = st;
        let Some(images) = images.as_ref() else { return asks };
        let mut cx = Cx { images, revealed, saving, rects, now: crate::calendar::time::now() };
        let mut area = egui::ScrollArea::vertical().id_salt(("social-scroll", tab.id, &aid, &feed.id)).auto_shrink([false, false]);
        if let Some(off) = applied {
            area = area.vertical_scroll_offset(off);
        }
        area.show(ui, |ui| {
            content_top = ui.cursor().min.y;
            column(ui, width, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                for (i, p) in posts.iter().enumerate() {
                    let (save, rect) = post_card(ui, &mut cx, p, i, width);
                    if save {
                        save_post = Some(p.clone());
                    }
                    let k = post_key(p);
                    if gaps.contains(&k) {
                        ui.vertical_centered(|ui| {
                            ui.horizontal(|ui| {
                                let (r, spinner) = spinner_button(ui, "Load more in between", None, true, filling);
                                cx.rects.push((format!("gap:{i}"), r.rect));
                                if let Some(at) = spinner {
                                    cx.rects.push((format!("gap-spinner:{i}"), at));
                                }
                                if r.on_hover_text("Some posts between these were not fetched").clicked() {
                                    fill_gap = Some(k.clone());
                                }
                            });
                        });
                    }
                    post_rects.push((k, rect));
                }
                if let Some(e) = &error {
                    inset(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(format!("Could not load posts: {e}")).color(theme::LINK_MISSING)).wrap());
                        let r = ui.button("Retry");
                        cx.rects.push(("retry".into(), r.rect));
                        retry = r.clicked();
                    });
                } else if loading {
                    ui.vertical_centered(|ui| {
                        ui.spinner();
                    });
                } else if loaded && posts.is_empty() {
                    ui.label(RichText::new("Nothing here yet.").color(theme::MUTED));
                } else if loaded && !has_more {
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("That is all for now.").size(12.5).color(theme::FAINT));
                    });
                }
                // the end of the list: when it is on screen, the next page is due
                let (end, _) = ui.allocate_exact_size(Vec2::new(width, 60.0), Sense::hover());
                near_end = ui.is_rect_visible(end);
                ui.add_space(20.0);
            });
        })
    };
    if let Some(t) = st.timelines.get_mut(&key) {
        // posts that arrived during the frame cannot exist (poll runs elsewhere); put the list back
        t.posts = posts;
    }
    // ---- keep the view still
    let top = out.inner_rect.min.y;
    let offset_used = top - content_top;
    // what the user scrolled during this frame (it applies to the next one)
    let user = out.state.clamped_offset().y - offset_used;
    if let Some((_, r)) = post_rects.iter().find(|(k, _)| *k == anchor.key) {
        let dy_now = r.min.y - top;
        if applied.is_none() && (dy_now - anchor.dy).abs() > 0.5 {
            // posts above it changed in height: put the view back (in a second pass of this frame
            // when egui allows it, else in the next frame)
            anchor.apply = Some(offset_used + dy_now - anchor.dy);
            ctx.request_discard("keeping the view still");
            ctx.request_repaint();
        }
    }
    if anchor.apply.is_none() {
        match post_rects.iter().find(|(_, r)| r.max.y > top + 0.5) {
            Some((k, r)) => {
                anchor.key = k.clone();
                anchor.dy = r.min.y - top - user;
            }
            None => anchor.key.clear(),
        }
    }
    st.anchors.insert(tab.id, anchor);
    st.post_rects = post_rects;
    st.list_rect = out.inner_rect;
    if show_new {
        st.show_waiting(&key);
        ctx.request_repaint();
    }
    if let Some(above) = fill_gap {
        st.fill_gap(&ctx, &entry, &feed, &above);
    }
    if retry {
        st.reload(&ctx, &entry, &feed);
    } else if near_end && has_more && !loading && error.is_none() {
        st.load_more(&ctx, &entry, &feed);
    }
    if let Some(p) = save_post {
        st.save(&ctx, &entry, &feed.id, &p);
    }
    if st.images.as_ref().map(|i| i.pending() > 0).unwrap_or(false) {
        ctx.request_repaint_after(std::time::Duration::from_millis(300));
    }
    asks
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: SOC-008
    #[test]
    fn post_text_becomes_pieces_with_links() {
        let s = segments("Hi [@ada](https://x.org/@ada), see https://e.com/a. [docs \\[v2\\]](https://d.org/) and [not a link");
        let text: String = s.iter().map(|p| p.0.as_str()).collect();
        assert_eq!(text, "Hi @ada, see https://e.com/a. docs [v2] and [not a link");
        let links: Vec<(&str, &str)> = s.iter().filter_map(|(t, u)| u.as_deref().map(|u| (t.as_str(), u))).collect();
        assert_eq!(links, [("@ada", "https://x.org/@ada"), ("https://e.com/a", "https://e.com/a"), ("docs [v2]", "https://d.org/")]);
        assert_eq!(segments("plain\nlines"), [("plain\nlines".to_string(), None)]);
        assert!(segments("").is_empty());
    }

    use crate::social::{demo, Account, Network};
    use eframe::egui::{Event as Ev, Modifiers, PointerButton, Pos2, RawInput};

    struct H {
        ctx: egui::Context,
        t: f64,
        /// the tab shown (a `Resolve` answer fills in its feed, as the app does)
        tab: TabCtx,
    }

    impl H {
        fn new() -> H {
            let ctx = egui::Context::default();
            crate::theme::install_fonts(&ctx);
            H { ctx, t: 0.0, tab: TabCtx { id: 1, feed: None, last: None, keys: true } }
        }
        fn frame(&mut self, events: Vec<Ev>, st: &mut SocialState, accounts: &[SocialAccountEntry]) -> Vec<Ask> {
            self.t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 4000.0))), time: Some(self.t), events, ..Default::default() };
            let mut asks = vec![];
            let tab = &self.tab;
            let mut out = self.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    asks = show(ui, st, accounts, tab);
                });
            });
            out.textures_delta.clear();
            let mut keep = vec![];
            for a in asks {
                match a {
                    Ask::Resolve(f) => self.tab.feed = Some(f),
                    a => keep.push(a),
                }
            }
            keep
        }
        fn click(&mut self, key: &str, st: &mut SocialState, accounts: &[SocialAccountEntry]) -> Vec<Ask> {
            let p = st.rects.iter().rev().find(|r| r.0 == key).unwrap_or_else(|| panic!("no {key}: {:?}", st.rects.iter().map(|r| &r.0).collect::<Vec<_>>())).1.center();
            let b = |pressed| Ev::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            let mut r = self.frame(vec![Ev::PointerMoved(p)], st, accounts);
            r.extend(self.frame(vec![b(true)], st, accounts));
            r.extend(self.frame(vec![b(false)], st, accounts));
            r.extend(self.frame(vec![], st, accounts));
            r
        }
    }

    fn demo_state() -> (SocialState, Vec<SocialAccountEntry>) {
        let mut st = SocialState::new(std::env::temp_dir().join(format!("shale-social-test-{}", std::process::id())));
        let accounts = vec![demo::account()];
        st.load_demo(&accounts[0]);
        (st, accounts)
    }

    // spec: SOC-006
    #[test]
    fn the_feed_shows_posts_hides_content_warnings_and_saves_without_network() {
        let (mut st, accounts) = demo_state();
        let mut h = H::new();
        h.frame(vec![], &mut st, &accounts);
        for k in ["account-combo", "feed-combo", "check-now", "accounts", "save:0", "open:0", "copy:0", "cw:2", "media:3:0", "media:3:1"] {
            assert!(st.rects.iter().any(|r| r.0 == k), "{k}: {:?}", st.rects.iter().map(|r| &r.0).collect::<Vec<_>>());
        }
        // a content warning hides the post until "Show" is clicked
        let y_hidden = st.rects.iter().find(|r| r.0 == "save:2").unwrap().1.min.y;
        h.click("cw:2", &mut st, &accounts);
        assert!(st.revealed.contains(&post_key(&demo::posts()[2])));
        assert!(st.rects.iter().find(|r| r.0 == "save:2").unwrap().1.min.y > y_hidden, "the text opened up");
        h.click("cw:2", &mut st, &accounts);
        assert!(st.revealed.is_empty());
        // the thread post: the button works on the whole thread, nothing goes to the network
        h.click("save:4", &mut st, &accounts);
        assert!(st.saving.contains(&post_key(&demo::posts()[4])), "spinner while it runs");
        for _ in 0..200 {
            st.poll();
            if !st.saved.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(st.saved.len(), 1);
        let (saved, network) = &st.saved[0];
        assert_eq!((saved.thread.len(), saved.media.len(), *network), (3, 1, "Mastodon"));
        assert!(st.saving.is_empty());
        // the account button asks for the dialog
        assert_eq!(h.click("accounts", &mut st, &accounts), vec![Ask::Accounts]);
        // with no account there is a friendly start
        let mut empty = SocialState::new(st.cache_dir.clone());
        h.frame(vec![], &mut empty, &[]);
        assert_eq!(h.click("add-account", &mut empty, &[]), vec![Ask::Accounts]);
    }

    // spec: SOC-009
    #[test]
    fn the_end_of_the_list_loads_the_next_page_once_and_an_error_waits_for_retry() {
        let mut st = SocialState::new(std::env::temp_dir().join(format!("shale-social-test2-{}", std::process::id())));
        // a server that refuses at once (nothing is sent anywhere)
        let e = SocialAccountEntry { account: Account::new(Network::Mastodon, "https://localhost:9", "ada@localhost", "Ada"), secret: "x".into() };
        let accounts = vec![e.clone()];
        st.feeds.insert(e.account.id.clone(), FeedsState::Ready(demo::feeds()));
        let key = (e.account.id.clone(), "home".to_string());
        st.timelines.insert(key.clone(), super::super::state::Timeline { posts: demo::posts(), cursor: Some("next".into()), loaded: true, ..Default::default() });
        let mut h = H::new();
        h.frame(vec![], &mut st, &accounts);
        assert!(st.timelines[&key].loading, "the end is on screen: the next page is asked for");
        h.frame(vec![], &mut st, &accounts);
        for _ in 0..300 {
            st.poll();
            if st.timelines[&key].error.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(st.timelines[&key].error.is_some() && !st.timelines[&key].loading, "a failed load ends with a message");
        h.frame(vec![], &mut st, &accounts);
        assert!(!st.timelines[&key].loading, "no new attempt until Retry is clicked");
        assert_eq!(st.timelines[&key].posts.len(), demo::posts().len(), "the posts stay");
        h.click("retry", &mut st, &accounts);
        assert!(st.timelines[&key].loading);
    }

    #[test]
    fn ages() {
        assert_eq!(age_text(1000, 1030), "now");
        assert_eq!(age_text(1000, 1000 + 5 * 60), "5m");
        assert_eq!(age_text(1000, 1000 + 3 * 3600), "3h");
        assert_eq!(age_text(1000, 1000 + 2 * 86_400), "2d");
        assert_eq!(age_text(0, 5), "");
    }
}
