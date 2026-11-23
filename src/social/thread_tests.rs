//! The thread view of a Feeds tab (SOC-045 .. SOC-049): opening the conversation of a post,
//! the tree, the highlighted post, the background job with its spinner and Retry, the tab
//! history, a new tab, and the index. The window runs headless like in `ui_tests.rs`; the
//! pretend server is `ui_tests::Script`. The Mastodon and Bluesky answers are checked in
//! `mastodon.rs` and `bluesky.rs`.

use super::state::post_key;
use super::tests::post;
use super::ui_tests::{home, wait_for, W};
use super::{thread_view_of, Media, MediaKind, Network, Post, ThreadView};
use crate::app::View;
use eframe::egui::{Event, Modifiers, PointerButton, Rect};

/// The conversation of the first post of the home feed: two posts above it, replies below it as a
/// tree (a chain of 8 levels under `c0`, and a second answer `r3`), listed out of order.
fn fixture(root: &Post) -> ThreadView {
    let root = Post { reply_count: 2, ..root.clone() };
    // recent, as the index keeps no old posts (SOC-032)
    let now = crate::calendar::time::now();
    let kid = |id: &str, who: &str, at: i64, parent: &str, replies: u32| Post { reply_count: replies, ..post(id, who, at, Some(parent)) };
    let mut below: Vec<Post> = (0..8).map(|i| kid(&format!("c{i}"), "carol", now - 3000 + i, &if i == 0 { "h0".to_string() } else { format!("c{}", i - 1) }, u32::from(i < 7))).collect();
    below.push(kid("r3", "dave", now - 2000, "h0", 0));
    below.reverse();
    thread_view_of(&root, vec![post("up1", "alice", now - 5000, None), post("up2", "bob", now - 4900, Some("up1"))], &below)
}

/// A window whose first home post is the one the conversation is about, and the server that answers for it.
fn window(tag: &str) -> W {
    let mut w = W::new(tag);
    let key = (home().account, "home".to_string());
    let t = w.app.social.timelines.get_mut(&key).unwrap();
    t.posts[0].reply_to = Some("up2".into());
    t.posts[0].reply_count = 2;
    t.posts[0].created_at = crate::calendar::time::now() - 4000;
    let view = fixture(&t.posts[0]);
    w.script.threads.lock().unwrap().insert("h0".into(), vec![Ok(view)]);
    w.frames(3);
    w
}

fn thread_ref(id: &str) -> View {
    View::Social(Some(super::FeedRef { thread: Some(id.into()), ..home() }))
}

fn ready(w: &W) -> bool {
    w.app.social.post_rects.len() > 3 && !w.app.social.rects.iter().any(|(k, _)| k == "thread-loading")
}

fn rect_of(w: &W, id: &str) -> Rect {
    let k = format!("{id}|");
    w.app.social.post_rects.iter().find(|r| r.0 == k).unwrap_or_else(|| panic!("post {id} not shown: {:?}", w.app.social.post_rects.iter().map(|r| &r.0).collect::<Vec<_>>())).1
}

fn shown(w: &W) -> Vec<String> {
    w.app.social.post_rects.iter().map(|r| r.0.trim_end_matches('|').to_string()).collect()
}

fn with_shift(w: &mut W, f: impl FnOnce(&mut W)) {
    w.frame(vec![Event::ModifiersChanged(Modifiers::SHIFT)]);
    f(w);
    w.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
}

fn middle_click(w: &mut W, key: &str) {
    let p = w.rect(key).center();
    let b = |pressed| Event::PointerButton { pos: p, button: PointerButton::Middle, pressed, modifiers: Modifiers::NONE };
    w.frame(vec![Event::PointerMoved(p)]);
    w.frame(vec![b(true)]);
    w.frame(vec![b(false)]);
    w.frames(3);
}

// spec: SOC-045, SOC-046
#[test]
fn soc_045_the_reply_count_opens_the_conversation_with_the_posts_above_the_tree_below_and_the_opened_post_highlighted() {
    let mut w = window("thread-open");
    assert_eq!(w.app.tabbar.tabs.len(), 1);
    w.click("replies:0");
    assert_eq!((w.app.tabbar.tabs.len(), w.view(0)), (1, thread_ref("h0")), "the conversation opens in this tab");
    assert_eq!(w.title(0), "Mastodon · Conversation");
    wait_for(&mut w, ready);
    w.frames(6);
    // the posts above it from the top down, the post, then the tree depth first, oldest first among siblings
    assert_eq!(shown(&w), ["up1", "up2", "h0", "c0", "c1", "c2", "c3", "c4", "c5", "c6", "c7", "r3"]);
    let ys: Vec<f32> = shown(&w).iter().map(|id| rect_of(&w, id).min.y).collect();
    assert!(ys.windows(2).all(|p| p[0] < p[1]), "the posts follow each other downwards: {ys:?}");
    // the opened post is highlighted, and the view is moved to it: the posts above it are above the window
    assert_eq!(w.wait_rect("highlight"), rect_of(&w, "h0"));
    assert!((rect_of(&w, "h0").min.y - w.app.social.list_rect.min.y).abs() < 8.0, "the opened post is at the top of the list");
    assert!(!w.app.social.rects.iter().any(|(k, _)| k == "replies:2"), "the opened post has no reply count to click");
    // replies are moved in by their level, up to 6 levels; deeper ones stay at the last indent
    let x = |id: &str| rect_of(&w, id).min.x;
    assert_eq!((x("up1"), x("up2"), x("h0")), (x("h0"), x("h0"), x("h0")), "the posts above it and the opened post are not moved in");
    let step = super::thread_ui::INDENT;
    for (i, level) in (0..8).zip(1..) {
        let want = x("h0") + step * level.min(super::thread_ui::MAX_LEVELS) as f32;
        assert!((x(&format!("c{i}")) - want).abs() < 1.0, "c{i} at level {level}: {} vs {want}", x(&format!("c{i}")));
    }
    assert!((x("r3") - x("h0") - step).abs() < 1.0, "a second answer to the post is at the first level");
    assert_eq!(x("c5"), x("c6"));
    assert_eq!(x("c6"), x("c7"));
    // the cards are the cards of the feed: Save, Open in browser and Copy link on each, the counts, "Show thread" on the others
    for k in ["save:5", "open:5", "copy:5", "thread:5", "replies:5"] {
        w.rect(k);
    }
    // the controls of a card sit inside it, also when it is moved in
    for (i, id) in shown(&w).iter().enumerate().take(8) {
        let (card, save) = (rect_of(&w, id), w.rect(&format!("save:{i}")));
        assert!(card.contains_rect(save), "{id}: {save:?} not in {card:?}");
    }
    assert!(!w.app.social.rects.iter().any(|(k, _)| k == "thread:2"), "the opened post does not offer itself");
    // one request for the whole conversation, and nothing was put into the feed's list
    assert_eq!(*w.script.thread_calls.lock().unwrap(), ["h0"]);
    assert_eq!(w.app.social.timelines[&(home().account, "home".to_string())].posts.len(), 40);
}

// spec: SOC-045
#[test]
fn soc_045_the_show_thread_button_opens_it_too_and_shift_or_middle_click_open_it_in_a_new_tab() {
    let mut w = window("thread-newtab");
    w.click("thread:0");
    assert_eq!((w.app.tabbar.tabs.len(), w.view(0)), (1, thread_ref("h0")));
    w.app.go_back();
    w.frames(3);
    assert_eq!(w.view(0), View::Social(Some(home())));
    // Shift+click on the count: a new tab, the first one stays on the feed
    with_shift(&mut w, |w| w.click("replies:0"));
    assert_eq!((w.app.tabbar.tabs.len(), w.app.tabbar.active), (2, 1));
    assert_eq!((w.view(0), w.view(1)), (View::Social(Some(home())), thread_ref("h0")));
    wait_for(&mut w, ready);
    assert!(shown(&w).contains(&"c0".to_string()));
    // a middle click on the button
    w.click_tab(0);
    middle_click(&mut w, "thread:0");
    assert_eq!((w.app.tabbar.tabs.len(), w.view(2)), (3, thread_ref("h0")));
    // a post that neither has replies nor replies to something does not offer the button, but its count can be clicked
    w.click_tab(0);
    w.frames(3);
    assert!(!w.app.social.rects.iter().any(|(k, _)| k == "thread:1"), "no replies, no parent: no Show thread");
    assert!(w.app.social.rects.iter().any(|(k, _)| k == "replies:1"));
}

// spec: SOC-047
#[test]
fn soc_047_the_conversation_is_fetched_in_the_background_with_a_spinner_and_the_window_never_waits() {
    let mut w = window("thread-spinner");
    let (tx, rx) = std::sync::mpsc::channel();
    *w.script.thread_gate.lock().unwrap() = Some(rx);
    w.click("replies:0");
    w.frames(5);
    // the request is held: the opened post is there, highlighted, with a spinner in place of the rest, and the window answers
    assert_eq!(shown(&w), ["h0"]);
    w.wait_rect("highlight");
    w.wait_rect("thread-spinner");
    w.wait_rect("thread-back");
    assert!(!w.app.social.rects.iter().any(|(k, _)| k == "thread-retry"));
    tx.send(()).unwrap();
    wait_for(&mut w, ready);
    assert_eq!(shown(&w).len(), 12);
    assert!(!w.app.social.rects.iter().any(|(k, _)| k == "thread-spinner"));
}

// spec: SOC-047
#[test]
fn soc_047_a_failed_request_shows_the_error_and_retry_asks_again() {
    let mut w = window("thread-retry");
    let view = w.script.threads.lock().unwrap()["h0"][0].clone();
    *w.script.threads.lock().unwrap().get_mut("h0").unwrap() = vec![Err("the server is down".into()), view];
    w.click("replies:0");
    wait_for(&mut w, |w| w.app.social.rects.iter().any(|(k, _)| k == "thread-retry"));
    assert_eq!(shown(&w), ["h0"], "the opened post stays");
    assert!(!w.app.social.rects.iter().any(|(k, _)| k == "thread-spinner"));
    w.click("thread-retry");
    wait_for(&mut w, ready);
    assert_eq!(shown(&w).len(), 12);
    assert_eq!(*w.script.thread_calls.lock().unwrap(), ["h0", "h0"]);
    assert!(!w.app.social.rects.iter().any(|(k, _)| k == "thread-retry"));
}

// spec: SOC-048
#[test]
fn soc_048_back_returns_to_the_feed_at_the_same_scroll_position_and_forward_to_the_conversation() {
    let mut w = window("thread-back");
    w.scroll(700.0);
    // the first post whose count can be clicked and is fully in the window
    let list = w.app.social.list_rect;
    let idx = (0..40).find(|i| w.app.social.rects.iter().find(|(k, _)| *k == format!("replies:{i}")).is_some_and(|(_, r)| r.min.y > list.min.y + 5.0 && r.max.y < list.max.y - 5.0)).expect("a post in view");
    assert!(idx > 3, "scrolled down to post {idx}");
    let before = w.y(idx);
    w.script.threads.lock().unwrap().insert(format!("h{idx}"), vec![Ok(thread_view_of(&w.app.social.timelines[&(home().account, "home".to_string())].posts[idx], vec![], &[]))]);
    w.click(&format!("replies:{idx}"));
    assert_eq!(w.view(0), thread_ref(&format!("h{idx}")));
    wait_for(&mut w, |w| w.app.social.rects.iter().any(|(k, _)| k == "highlight"));
    // the button in the view goes back one step
    w.click("thread-back");
    w.frames(4);
    assert_eq!(w.view(0), View::Social(Some(home())));
    assert!((w.y(idx) - before).abs() < 1.0, "the feed is where it was: {} vs {before}", w.y(idx));
    w.app.go_forward();
    w.frames(3);
    assert_eq!(w.view(0), thread_ref(&format!("h{idx}")));
    w.app.go_back();
    w.frames(4);
    assert!((w.y(idx) - before).abs() < 1.0);
    // a conversation is not a feed to come back to after a restart: the saved tab is the feed
    w.app.go_forward();
    assert_eq!(w.app.open_feed_refs(), [home()]);
}

// spec: SOC-048
#[test]
fn soc_048_a_click_on_another_post_of_the_conversation_opens_that_one_as_another_step() {
    let mut w = window("thread-nested");
    let c1 = post("c1", "carol", 1101, Some("c0"));
    w.script.threads.lock().unwrap().insert("c1".into(), vec![Ok(thread_view_of(&c1, vec![post("c0", "carol", 1100, None)], &[post("z9", "erin", 1500, Some("c1"))]))]);
    w.click("replies:0");
    wait_for(&mut w, ready);
    w.frames(6);
    // the post c1 is the fifth card: up1, up2, h0, c0, c1
    w.click("replies:4");
    assert_eq!((w.app.tabbar.tabs.len(), w.view(0)), (1, thread_ref("c1")));
    wait_for(&mut w, |w| shown(w) == ["c0", "c1", "z9"]);
    assert_eq!(w.wait_rect("highlight"), rect_of(&w, "c1"));
    w.app.go_back();
    w.frames(3);
    assert_eq!(w.view(0), thread_ref("h0"));
    assert_eq!(shown(&w).len(), 12, "the first conversation is as it was");
    w.app.go_back();
    assert_eq!(w.view(0), View::Social(Some(home())));
}

// spec: SOC-049
#[test]
fn soc_049_the_posts_of_the_conversation_are_put_into_the_index() {
    let mut w = window("thread-index");
    assert_eq!(w.app.social.index.count(None), 0);
    w.click("replies:0");
    wait_for(&mut w, ready);
    // up1, up2, h0, c0..c7 and r3
    wait_for(&mut w, |w| w.app.social.index.count(None) == 12);
    let hit = w.app.social.index.search_wild("text of c7", 5);
    assert_eq!((hit.len(), hit[0].account.as_str(), hit[0].feed.as_str()), (1, home().account.as_str(), "home"));
    // a reply is indexed with the text of its parent (SOC-038): the search for the top post also finds the one below it
    assert_eq!(w.app.social.index.search_wild("text of up1", 5).len(), 2);
}

// spec: SOC-046
#[test]
fn soc_046_replies_the_server_left_out_say_so_with_open_in_browser() {
    let mut w = window("thread-cut");
    let key = (home().account, "home".to_string());
    let root = w.app.social.timelines[&key].posts[0].clone();
    // the answer of a server that stopped at its depth limit: the post counts more replies than the tree holds
    let counted = Post { reply_count: 5, ..post("deep", "carol", 1100, Some("h0")) };
    let cut = thread_view_of(&root, vec![], &[counted]);
    assert!(cut.cut);
    w.script.threads.lock().unwrap().insert("h0".into(), vec![Ok(cut)]);
    w.click("replies:0");
    wait_for(&mut w, |w| w.app.social.rects.iter().any(|(k, _)| k == "thread-cut-open"));
    assert_eq!(shown(&w), ["h0", "deep"]);
    // a complete conversation does not say it
    let whole = thread_view_of(&root, vec![], &[post("x", "carol", 1100, Some("h0")), post("y", "carol", 1101, Some("h0"))]);
    assert!(!whole.cut);
    w.script.threads.lock().unwrap().insert("h0".into(), vec![Ok(whole)]);
    w.click("thread-back");
    w.click("replies:0");
    wait_for(&mut w, |w| shown(w) == ["h0", "x", "y"]);
    assert!(!w.app.social.rects.iter().any(|(k, _)| k == "thread-cut-open"));
    // no replies at all
    let key = "h0".to_string();
    w.script.threads.lock().unwrap().insert(key, vec![Ok(thread_view_of(&Post { reply_count: 0, ..root }, vec![], &[]))]);
    w.click("thread-back");
    w.click("replies:0");
    wait_for(&mut w, |w| shown(w) == ["h0"] && !w.app.social.rects.iter().any(|(k, _)| k == "thread-loading"));
}

// spec: SOC-047
#[test]
fn soc_047_replies_are_cards_like_the_feed_with_pictures_the_viewer_content_warnings_and_save() {
    let mut w = window("thread-cards");
    let key = (home().account, "home".to_string());
    let root = w.app.social.timelines[&key].posts[0].clone();
    let mut reply = post("pic", "carol", 1100, Some("h0"));
    reply.network = Network::Bluesky;
    reply.media = vec![Media { kind: MediaKind::Image, url: "https://files.test/a.png".into(), preview_url: String::new(), alt: "a cat".into(), file_name_hint: "a.png".into(), blob_cid: None }];
    let mut hidden = post("cw", "dave", 1200, Some("h0"));
    hidden.content_warning = Some("spoilers".into());
    let view = thread_view_of(&Post { reply_count: 2, ..root }, vec![], &[reply, hidden]);
    w.script.threads.lock().unwrap().insert("h0".into(), vec![Ok(view)]);
    w.click("replies:0");
    wait_for(&mut w, |w| shown(w) == ["h0", "pic", "cw"]);
    // the picture opens in the viewer (SOC-044)
    assert!(w.app.social.viewer.is_none());
    w.click("media:1:0");
    assert!(w.app.social.viewer.as_ref().is_some_and(|v| v.tab == w.app.tabbar.tabs[0].id));
    w.frame(vec![Event::Key { key: eframe::egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    w.frames(3);
    assert!(w.app.social.viewer.is_none());
    // a content warning is shown and can be opened
    let key = post_key(&post("cw", "dave", 1200, Some("h0")));
    assert!(!w.app.social.revealed.contains(&key));
    // (the cards hold a row of buttons: the post with the warning is further down)
    w.scroll(250.0);
    w.frames(3);
    w.click("cw:2");
    assert!(w.app.social.revealed.contains(&key));
    // Save to vault works from here: the Import dialog opens with the post
    w.click("save:1");
    wait_for(&mut w, |w| matches!(w.app.modal, Some(crate::app::Modal::Import(_))));
}

// spec: SOC-046
#[test]
fn soc_046_the_opened_post_stays_put_when_the_height_of_a_post_above_it_changes_later_until_the_user_scrolls() {
    let mut w = window("thread-still");
    // the first post above it has a content warning: its height changes when it is opened (as a picture's does when it arrives)
    let mut view = w.script.threads.lock().unwrap()["h0"][0].clone().unwrap();
    view.ancestors[0].content_warning = Some("long".into());
    view.ancestors[0].text_markdown = "a long text. ".repeat(80);
    let up1 = post_key(&view.ancestors[0]);
    w.script.threads.lock().unwrap().insert("h0".into(), vec![Ok(view)]);
    w.click("replies:0");
    wait_for(&mut w, ready);
    w.frames(6);
    let y0 = rect_of(&w, "h0").min.y;
    assert!((y0 - w.app.social.list_rect.min.y).abs() < 8.0, "the opened post is at the top");
    for open in [true, false, true] {
        if open {
            w.app.social.revealed.insert(up1.clone());
        } else {
            w.app.social.revealed.clear();
        }
        for i in 0..5 {
            w.frames(1);
            assert!((rect_of(&w, "h0").min.y - y0).abs() < 1.0, "open={open} frame {i}: {} vs {y0}", rect_of(&w, "h0").min.y);
        }
    }
    // once the user scrolls, the view holds the post at the top of the window instead
    w.scroll(300.0);
    let top = w.app.social.post_rects.iter().find(|(_, r)| r.max.y > w.app.social.list_rect.min.y + 1.0).unwrap().clone();
    assert!(rect_of(&w, "h0").min.y < y0 - 100.0);
    w.app.social.revealed.clear();
    w.frames(5);
    let again = w.app.social.post_rects.iter().find(|(k, _)| *k == top.0).unwrap().1;
    assert!((again.min.y - top.1.min.y).abs() < 1.0, "{} vs {}", again.min.y, top.1.min.y);
}
