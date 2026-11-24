//! Feeds as a view of the right sidebar (LAY-034, LAY-037, ADR-0038): a compact companion of the
//! Feeds tab, not its column of cards squeezed into a narrow width. It has the accounts, a
//! feed picker (the home timeline first) and the posts of the chosen feed as small rows: the
//! author, the age, the beginning of the text and the number of pictures. A click on a post asks
//! the window to open its conversation in the Feeds tab. Reacting, replying, saving, composing,
//! the pictures and the settings are in the tab only.
//!
//! The view state (the chosen account and feed, the scroll) is the pane's own, so the sidebar
//! can show the home timeline while the tab shows a list (LAY-031). The data is the tab's: the
//! feed lists, the posts loaded and the fetches of the one `SocialState`. The pane asks for the
//! first load of what it shows exactly like the tab does (`ensure_feeds`, `ensure_timeline`; they
//! do nothing when the data is there or on its way, so nothing is fetched twice). It never loads
//! older pages and never checks for new posts: that is the tab's, and the timer checks only the
//! feeds that are open in a tab.

use super::feed_ui::{account_label, age_text, short};
use super::state::{FeedsState, SocialState};
use super::{markup, Feed, FeedRef, MediaKind, Post, SocialAccountEntry};
use crate::app::muted;
use crate::theme;
use eframe::egui::{self, text::LayoutJob, Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

/// The height of a post row, in points: the author line and two lines of text.
pub(crate) const ROW_H: f32 = 64.0;
/// The longest the text of a post is cut to before the two lines cut it again.
const TEXT_CHARS: usize = 200;

/// The state of the Feeds view of the right sidebar. It is not saved: a new window shows the
/// first feed of the first account.
#[derive(Default)]
pub(crate) struct SocialPane {
    /// the chosen account (an id); None: the first one
    pub account: Option<String>,
    /// the chosen feed (an id) of that account; None: its first feed
    pub feed: Option<String>,
    /// how far the list is scrolled, in points (the last frame's)
    pub scroll: f32,
    /// the account and feed drawn in the last frame (for the UI tests)
    pub shown: Option<(String, String)>,
    /// controls drawn in the last frame, by name (for the UI tests)
    pub rects: Vec<(String, Rect)>,
}

impl SocialPane {
    /// Draw the pane into the free space of `ui`. Returns the post that was clicked, with the
    /// feed it was in: the window opens its conversation in the Feeds tab.
    pub fn show(&mut self, ui: &mut Ui, st: &mut SocialState, accounts: &[SocialAccountEntry]) -> Option<(FeedRef, Post)> {
        self.rects.clear();
        self.shown = None;
        if accounts.is_empty() {
            ui.label(muted("No feed account yet. Choose Feeds in the ribbon and add a Mastodon or Bluesky account."));
            return None;
        }
        let ctx = ui.ctx().clone();
        let mut rects = std::mem::take(&mut self.rects);
        // the account: the chosen one while it exists, else the first
        let current = self.account.clone().filter(|id| accounts.iter().any(|e| e.account.id == *id)).unwrap_or_else(|| accounts[0].account.id.clone());
        if accounts.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                for e in accounts {
                    let r = ui.selectable_label(e.account.id == current, account_label(e));
                    rects.push((format!("feedpane:account:{}", e.account.id), r.rect));
                    if r.clicked() && e.account.id != current {
                        self.account = Some(e.account.id.clone());
                        self.feed = None;
                    }
                }
            });
        }
        let account = self.account.clone().filter(|id| accounts.iter().any(|e| e.account.id == *id)).unwrap_or(current);
        let entry = accounts.iter().find(|e| e.account.id == account).expect("the account exists").clone();
        st.ensure_feeds(&ctx, &entry);
        let list = match st.feeds.get(&account) {
            Some(FeedsState::Ready(list)) => list.clone(),
            Some(FeedsState::Failed(e)) => {
                ui.add(egui::Label::new(RichText::new(format!("Could not load the feeds: {e}")).color(theme::palette().link_missing)).wrap());
                ui.label(muted("Reload them in the Feeds tab."));
                self.rects = rects;
                return None;
            }
            _ => {
                ui.label(muted("Loading the feeds…"));
                self.rects = rects;
                return None;
            }
        };
        let Some(feed) = self.feed.as_ref().and_then(|id| list.iter().find(|f| f.id == *id)).or(list.first()).cloned() else {
            ui.label(muted("This account has no feeds."));
            self.rects = rects;
            return None;
        };
        if list.len() > 1 {
            let c = egui::ComboBox::from_id_salt("feeds-pane-feed").selected_text(feed.title.clone()).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
                for f in &list {
                    let r = ui.selectable_label(f.id == feed.id, &f.title);
                    rects.push((format!("feedpane:feed:{}", f.id), r.rect));
                    if r.clicked() {
                        self.feed = Some(f.id.clone());
                    }
                }
            });
            rects.push(("feedpane:feed".into(), c.response.rect));
        }
        self.shown = Some((account.clone(), feed.id.clone()));
        st.ensure_timeline(&ctx, &entry, &feed);
        ui.separator();
        let clicked = self.posts(ui, st, &entry, &feed, &mut rects);
        self.rects = rects;
        clicked
    }

    /// The rows of the feed, or why there are none.
    fn posts(&mut self, ui: &mut Ui, st: &SocialState, entry: &SocialAccountEntry, feed: &Feed, rects: &mut Vec<(String, Rect)>) -> Option<(FeedRef, Post)> {
        let key = (entry.account.id.clone(), feed.id.clone());
        let Some(t) = st.timelines.get(&key) else {
            ui.label(muted("Loading the feed…"));
            return None;
        };
        if t.posts.is_empty() {
            if let Some(e) = &t.error {
                ui.add(egui::Label::new(RichText::new(format!("Could not load posts: {e}")).color(theme::palette().link_missing)).wrap());
                ui.label(muted("Retry in the Feeds tab."));
            } else if t.loaded {
                ui.label(muted(if feed.id == super::notifications::NOTIFICATIONS { "No notifications yet." } else { "Nothing here yet." }));
            } else {
                ui.label(muted("Loading the feed…"));
            }
            return None;
        }
        let mut clicked = None;
        let now = crate::calendar::time::now();
        ui.spacing_mut().item_spacing.y = 0.0;
        let out = egui::ScrollArea::vertical().id_salt(("feeds-pane-list", &key.0, &key.1)).auto_shrink([false, false]).show_rows(ui, ROW_H, t.posts.len(), |ui, range| {
            for i in range {
                let p = &t.posts[i];
                let r = post_row(ui, p, now);
                rects.push((format!("feedpane:row:{i}"), r.rect));
                if r.clicked() {
                    clicked = Some((FeedRef::new(key.0.clone(), key.1.clone()), p.clone()));
                }
            }
            if t.cursor.is_some() {
                ui.add_space(6.0);
                ui.label(muted("Older posts are in the Feeds tab."));
            }
        });
        self.scroll = out.state.clamped_offset().y;
        clicked
    }
}

/// How many pictures or attachments a post has, as words: "2 pictures", "1 attachment"; "" for none.
fn media_words(p: &Post) -> String {
    let n = p.media.len();
    match (n, p.media.iter().all(|m| m.kind == MediaKind::Image)) {
        (0, _) => String::new(),
        (1, true) => "1 picture".into(),
        (n, true) => format!("{n} pictures"),
        (1, false) => "1 attachment".into(),
        (n, false) => format!("{n} attachments"),
    }
}

/// What the row shows of the text: the content warning instead of a text behind one (the pane has
/// no way to open it), else the text without its markup, cut short.
fn row_text(p: &Post) -> String {
    match &p.content_warning {
        Some(cw) => format!("Content warning: {cw}"),
        None => short(&markup::plain_text(&p.text_markdown), TEXT_CHARS),
    }
}

/// One post as a row: the author and the age, the pictures on the right, two lines of text.
fn post_row(ui: &mut Ui, p: &Post, now: i64) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let pal = theme::palette();
    let painter = ui.painter_at(rect);
    if resp.hovered() {
        painter.rect_filled(rect, 0, pal.bg_hover);
    }
    painter.line_segment([Pos2::new(rect.min.x, rect.max.y), Pos2::new(rect.max.x, rect.max.y)], Stroke::new(1.0, pal.border));
    let x0 = rect.min.x + 8.0;
    let mut right = rect.max.x - 8.0;
    let words = media_words(p);
    if !words.is_empty() {
        let g = painter.layout_no_wrap(words, crate::fonts::font(11.0), pal.muted);
        right -= g.size().x;
        painter.galley(Pos2::new(right, rect.min.y + 8.0), g, Color32::WHITE);
        right -= 8.0;
    }
    // the author, the age, and who boosted it
    let mut head = LayoutJob::default();
    head.wrap.max_width = (right - x0).max(20.0);
    head.wrap.max_rows = 1;
    let fmt = |size: f32, color: Color32| egui::TextFormat { font_id: crate::fonts::font(size), color, ..Default::default() };
    head.append(p.author.name(), 0.0, fmt(13.0, pal.text_strong));
    let age = age_text(p.created_at, now);
    if !age.is_empty() {
        head.append(&format!("  {age}"), 0.0, fmt(12.0, pal.muted));
    }
    if let Some(b) = &p.reposted_by {
        head.append(&format!("  🔁 {}", b.name()), 0.0, fmt(12.0, pal.faint));
    }
    let g = painter.layout_job(head);
    painter.galley(Pos2::new(x0, rect.min.y + 6.0), g, Color32::WHITE);
    let mut body = LayoutJob::simple(row_text(p), crate::fonts::font(12.5), pal.text, (rect.max.x - 8.0 - x0).max(20.0));
    body.wrap.max_rows = 2;
    let g = painter.layout_job(body);
    painter.galley(Pos2::new(x0, rect.min.y + 26.0), g, Color32::WHITE);
    resp
}
