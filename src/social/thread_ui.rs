//! The thread view of a Feeds tab: the conversation around one post (SOC-045). The posts above it
//! (what it replies to), the post itself highlighted, and the replies below it as a tree, drawn
//! with the same card as the feed (`feed_ui::post_card`). It is shown instead of the feed when the
//! tab's `FeedRef` names a post; the tab's history makes Back return to the feed.

use super::feed_ui::{column, column_width, do_act, inset, keep_still, list_card, open_web, show_viewer, spinner_button, Anchor, Ask, CardAct, Cx, Look, TabCtx};
use super::images::ImageCache;
use super::state::{post_key, SocialState};
use super::thread::ThreadStatus;
use super::{FeedRef, Post, SocialAccountEntry};
use crate::theme;
use eframe::egui::{self, Rect, RichText, Ui};

/// How far a reply is moved in for each level of the tree, and the deepest level that is moved
/// (deeper ones stay at that indent).
pub const INDENT: f32 = 18.0;
pub const MAX_LEVELS: usize = 6;
/// The narrowest card a deep reply is left with.
const MIN_CARD: f32 = 220.0;

/// One card of the view.
struct Item {
    post: Post,
    /// levels of the tree this post is moved in (0: not moved)
    level: usize,
    highlight: bool,
}

/// The cards from the data: ancestors, the opened post, the replies.
fn items(root: &Post, view: Option<&super::ThreadView>) -> Vec<Item> {
    let Some(v) = view else { return vec![Item { post: root.clone(), level: 0, highlight: true }] };
    let mut out: Vec<Item> = v.ancestors.iter().map(|p| Item { post: p.clone(), level: 0, highlight: false }).collect();
    out.push(Item { post: v.post.clone(), level: 0, highlight: true });
    out.extend(v.replies.iter().map(|r| Item { post: r.post.clone(), level: r.depth + 1, highlight: false }));
    out
}

/// The whole view of a tab that shows a conversation.
pub fn show(ui: &mut Ui, st: &mut SocialState, entry: &SocialAccountEntry, tab: &TabCtx, f: &FeedRef) -> Vec<Ask> {
    let mut asks = vec![];
    let ctx = ui.ctx().clone();
    st.rects.clear();
    st.post_rects.clear();
    let tab_rect = ui.max_rect();
    let width = column_width(ui.available_width());
    let aid = entry.account.id.clone();
    let id = f.thread.clone().unwrap_or_default();
    let key = (aid.clone(), id.clone());
    st.fetch_thread(&ctx, entry, &f.feed, &id);
    let snapshot = st.threads.map.get(&key).map(|t| {
        let view = if let ThreadStatus::Ready(v) = &t.status { Some(v.clone()) } else { None };
        let error = if let ThreadStatus::Failed(e) = &t.status { Some(e.clone()) } else { None };
        (t.post.clone(), view, error)
    });
    // ---- the bar above the conversation
    ui.add_space(8.0);
    let mut back = false;
    column(ui, width, |ui| {
        ui.horizontal(|ui| {
            let r = ui.button("← Back").on_hover_text("Back to where this was opened from");
            st.rects.push(("thread-back".into(), r.rect));
            back = r.clicked();
            ui.label(RichText::new("Conversation").strong().size(crate::fonts::px(15.0)).color(theme::palette().text_strong));
            if snapshot.as_ref().is_some_and(|s| s.1.is_none() && s.2.is_none()) {
                ui.spinner();
            }
        });
    });
    if back {
        asks.push(Ask::Back);
    }
    ui.add_space(6.0);
    let Some((root, view, error)) = snapshot else {
        column(ui, width, |ui| {
            ui.label(RichText::new("This conversation is not available any more. Go back and open it again.").color(theme::palette().muted));
        });
        return asks;
    };
    let list = items(&root, view.as_deref());
    let mut save_post: Option<Post> = None;
    let mut open_thread: Option<(Post, bool)> = None;
    let mut acted: Option<(Post, CardAct)> = None;
    let mut retry = false;
    let mut post_rects: Vec<(String, Rect)> = vec![];
    // the opened post is held at the top of the view while posts above it change in height (a
    // picture arrives, the ancestors come in); when the user scrolls, the anchor moves with the view
    let mut anchor = st.threads.anchors.remove(&tab.id).filter(|a| a.scope == key).unwrap_or_else(|| Anchor { scope: key.clone(), key: post_key(&root), ..Default::default() });
    let applied = anchor.apply.take();
    let mut content_top = 0.0;
    let out = {
        let SocialState { images, revealed, saving, rects, viewer, threads, writes, .. } = st;
        let Some(images) = images.as_ref() else { return asks };
        let mut cx = Cx { images, revealed, saving, rects, now: crate::calendar::time::now(), tab: tab.id, viewer, account: &aid, busy: &writes.busy };
        let heights = &mut threads.heights;
        if heights.len() > 50_000 {
            heights.clear();
        }
        let mut area = egui::ScrollArea::vertical().id_salt(("social-thread", tab.id, &aid, &id)).auto_shrink([false, false]);
        if let Some(off) = applied {
            area = area.vertical_scroll_offset(off);
        }
        area.show(ui, |ui| {
            content_top = ui.cursor().min.y;
            column(ui, width, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                let (near_from, near_to) = (ui.clip_rect().min.y - super::feed_ui::FAR, ui.clip_rect().max.y + super::feed_ui::FAR);
                for (i, item) in list.iter().enumerate() {
                    let indent = (item.level.min(MAX_LEVELS) as f32 * INDENT).min((width - MIN_CARD).max(0.0));
                    let look = Look { idx: i, width: width - indent, highlight: item.highlight, show_thread: !item.highlight };
                    let card = list_card(ui, &mut cx, heights, &item.post, look, indent, (near_from, near_to));
                    if card.save {
                        save_post = Some(item.post.clone());
                    }
                    if let Some(new_tab) = card.thread {
                        open_thread = Some((item.post.clone(), new_tab));
                    }
                    if let Some(a) = card.act {
                        acted = Some((item.post.clone(), a));
                    }
                    if item.highlight {
                        cx.rects.push(("highlight".into(), card.rect));
                    }
                    post_rects.push((post_key(&item.post), card.rect));
                }
                if let Some(e) = &error {
                    inset(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(format!("Could not load the conversation: {e}")).color(theme::palette().link_missing)).wrap());
                        let r = ui.button("Retry");
                        cx.rects.push(("thread-retry".into(), r.rect));
                        retry = r.clicked();
                    });
                } else if let Some(v) = &view {
                    if v.cut {
                        inset(ui, |ui| {
                            ui.add(egui::Label::new(RichText::new("Some replies are not shown here: the server cut its answer off, or they are hidden from this account.").color(theme::palette().muted)).wrap());
                            let r = ui.button("Open in browser");
                            cx.rects.push(("thread-cut-open".into(), r.rect));
                            if r.on_hover_text("Read the rest on the server's page").clicked() {
                                open_web(&v.post.url);
                            }
                        });
                    } else if v.replies.is_empty() {
                        ui.label(RichText::new("No replies yet.").color(theme::palette().muted));
                    }
                } else {
                    ui.vertical_centered(|ui| {
                        let (r, spinner) = spinner_button(ui, "Loading the conversation…", None, false, true);
                        cx.rects.push(("thread-loading".into(), r.rect));
                        if let Some(at) = spinner {
                            cx.rects.push(("thread-spinner".into(), at));
                        }
                    });
                }
                ui.add_space(20.0);
            });
        })
    };
    st.post_rects = post_rects;
    st.list_rect = out.inner_rect;
    keep_still(&ctx, &mut anchor, &st.post_rects, &out, content_top, applied);
    st.threads.anchors.insert(tab.id, anchor);
    if retry {
        st.threads.retry(&aid, &id);
    }
    if let Some(p) = save_post {
        st.save(&ctx, entry, &f.feed, &p);
    }
    if let Some((post, new_tab)) = open_thread {
        asks.push(Ask::Thread { feed: f.feed_only(), post: Box::new(post), new_tab });
    }
    if let Some((post, act)) = acted {
        do_act(&ctx, st, entry, tab.id, &f.feed, &post, act);
    }
    show_viewer(&ctx, tab_rect, st, tab);
    super::compose_ui::show(&ctx, tab_rect, st, entry, tab);
    if st.images.as_ref().map(ImageCache::pending).is_some_and(|n| n > 0) {
        ctx.request_repaint_after(std::time::Duration::from_millis(300));
    }
    asks
}
