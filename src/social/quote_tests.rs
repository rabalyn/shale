//! The buttons of a quoted post (SOC-060) and fresh reactions in the post index (SOC-059): the quote inside
//! a card has its own Reply, Boost, Favourite, Quote, Show thread, Save, Open and Copy buttons that
//! act on the quoted post; a reaction is also written into the search index. The window runs
//! headless like in `ui_tests.rs`.

use super::actions::{Reacted, Reaction};
use super::state::post_key;
use super::tests::post;
use super::ui_tests::{home, wait_for, W};
use super::{thread_view_of, Post};
use eframe::egui::Rect;

/// A window whose first post (h0) quotes the post q0 (which has replies, so it offers its thread).
fn window(tag: &str) -> W {
    let mut w = W::new(tag);
    let key = (home().account, "home".to_string());
    let mut q = post("q0", "bob", crate::calendar::time::now() - 7200, None);
    q.reply_count = 2;
    q.like_count = 5;
    w.app.social.timelines.get_mut(&key).unwrap().posts[0].quote = Some(Box::new(q));
    w.frames(3);
    w
}

fn quoted(w: &W) -> Post {
    *card(w).quote.unwrap()
}

fn card(w: &W) -> Post {
    let key = (home().account, "home".to_string());
    w.app.social.timelines[&key].posts[0].clone()
}

fn size(r: Rect) -> (f32, f32) {
    (r.width().round(), r.height().round())
}

// spec: SOC-060
#[test]
fn soc_060_the_quoted_post_has_the_buttons_of_a_card_and_they_act_on_the_quoted_post() {
    let mut w = window("quote-buttons");
    for k in ["qreply:0", "qboost:0", "qlike:0", "qquote:0", "qthread:0", "qsave:0", "qopen:0", "qcopy:0", "reply:0", "boost:0", "like:0", "quote:0", "save:0"] {
        w.rect(k);
    }
    // favourite and boost
    w.click("qlike:0");
    wait_for(&mut w, |w| quoted(w).mine.liked);
    assert_eq!((quoted(&w).like_count, card(&w).mine.liked, card(&w).like_count), (6, false, 0), "the quote changed, the card did not");
    w.click("qboost:0");
    wait_for(&mut w, |w| quoted(w).mine.boosted);
    assert!(!card(&w).mine.boosted);
    // pressed again: undone
    w.click("qlike:0");
    wait_for(&mut w, |w| !quoted(w).mine.liked);
    assert_eq!(*w.script.reacts.lock().unwrap(), [("q0".to_string(), Reaction::Like, true), ("q0".to_string(), Reaction::Boost, true), ("q0".to_string(), Reaction::Like, false)]);
    // reply and quote write about the quoted post
    w.click("qreply:0");
    assert_eq!(w.app.social.compose.as_ref().and_then(|c| c.reply.as_ref()).map(|p| p.id.as_str()), Some("q0"));
    w.app.social.compose = None;
    w.frames(2);
    w.click("qquote:0");
    assert_eq!(w.app.social.compose.as_ref().and_then(|c| c.quote.as_ref()).map(|p| p.id.as_str()), Some("q0"));
    w.app.social.compose = None;
    w.frames(2);
    // copy link and open in browser are there and send nothing
    assert!(w.script.sends.lock().unwrap().is_empty());
    // save: the quoted post is the one that is saved (a save puts the post into the index, SOC-037)
    let q = quoted(&w);
    w.click("qsave:0");
    wait_for(&mut w, |w| w.app.social.index.contains(&home().account, &post_key(&q)));
}

// spec: SOC-060
#[test]
fn soc_060_show_thread_on_the_quoted_post_opens_its_conversation_in_the_tab() {
    let mut w = window("quote-open-thread");
    w.click("qthread:0");
    w.frames(2);
    assert_eq!(w.view(0), crate::app::View::Social(Some(super::FeedRef { thread: Some("q0".into()), ..super::FeedRef::new(home().account, "home") })));
    wait_for(&mut w, |w| !w.script.thread_calls.lock().unwrap().is_empty());
    assert_eq!(*w.script.thread_calls.lock().unwrap(), ["q0"], "the conversation of the quoted post is asked for, not the card's");
}

// spec: SOC-060, SOC-042
#[test]
fn soc_060_a_reaction_to_the_quoted_post_shows_a_spinner_in_its_button_which_keeps_its_size() {
    let mut w = window("quote-spinner");
    let (tx, rx) = std::sync::mpsc::channel();
    *w.script.react_gate.lock().unwrap() = Some(rx);
    w.script.react_answers.lock().unwrap().push(Ok(Reacted { on: true, record: None, count: Some(9) }));
    let (like, boost, save) = (w.rect("qlike:0"), w.rect("qboost:0"), w.rect("qsave:0"));
    w.click("qlike:0");
    w.frames(2);
    assert!(w.app.social.rects.iter().any(|r| r.0 == "qlike-spinner:0"), "the spinner shows in the button");
    assert!(!w.app.social.rects.iter().any(|r| r.0 == "like-spinner:0"), "not in the card's button");
    assert_eq!((size(w.rect("qlike:0")), w.rect("qboost:0"), w.rect("qsave:0")), (size(like), boost, save), "nothing moves");
    w.click("qlike:0");
    assert_eq!(w.script.reacts.lock().unwrap().len(), 1, "not twice while it runs");
    tx.send(()).unwrap();
    wait_for(&mut w, |w| quoted(w).mine.liked);
    w.frames(3);
    assert_eq!(quoted(&w).like_count, 9);
    assert!(!w.app.social.rects.iter().any(|r| r.0 == "qlike-spinner:0"));
    assert_eq!((size(w.rect("qlike:0")), w.rect("qsave:0")), (size(like), save), "the same size with and without the spinner");
}

// spec: SOC-060
#[test]
fn soc_060_the_quoted_post_has_its_buttons_in_the_conversation_too() {
    let mut w = window("quote-thread");
    let key = (home().account, "home".to_string());
    let root = w.app.social.timelines[&key].posts[0].clone();
    let mut reply = post("r1", "carol", crate::calendar::time::now() - 100, Some("h0"));
    reply.quote = Some(Box::new(post("q1", "dave", 1, None)));
    let view = thread_view_of(&root, vec![], &[reply]);
    w.script.threads.lock().unwrap().insert("h0".into(), vec![Ok(view)]);
    w.app.open_thread(home(), &root, false);
    w.frames(3);
    wait_for(&mut w, |w| w.app.social.rects.iter().any(|r| r.0 == "qlike:1"));
    // the opened post (highlighted) has no "Show thread"; its quote has the rest
    for k in ["qreply:0", "qboost:0", "qlike:0", "qquote:0", "qsave:0", "qopen:0", "qcopy:0", "qlike:1"] {
        w.rect(k);
    }
    assert!(!w.app.social.rects.iter().any(|r| r.0 == "qthread:0"));
    w.click("qlike:1");
    let liked = |w: &W| match &w.app.social.threads.map[&(home().account, "h0".to_string())].status {
        super::thread::ThreadStatus::Ready(v) => v.replies[0].post.quote.as_ref().unwrap().mine.liked,
        _ => false,
    };
    wait_for(&mut w, liked);
    assert_eq!(w.script.reacts.lock().unwrap()[0].0, "q1");
    w.click("qboost:0");
    wait_for(&mut w, |w| w.script.reacts.lock().unwrap().iter().any(|r| r.0 == "q0" && r.1 == Reaction::Boost));
}

// spec: SOC-059
#[test]
fn soc_059_a_reaction_changes_the_post_in_the_index_and_a_reopened_hit_shows_it() {
    let mut w = W::new("index-fresh");
    let acct = home().account;
    let key = (acct.clone(), "home".to_string());
    let mut p = w.app.social.timelines[&key].posts[3].clone();
    p.text_markdown = "a quince".into();
    p.created_at = crate::calendar::time::now() - 3600;
    w.app.social.timelines.get_mut(&key).unwrap().posts[3] = p.clone();
    w.app.social.index.add(&acct, "home", std::slice::from_ref(&p));
    let hit = |w: &W| w.app.social.index.search_wild("quince", 5).remove(0);
    assert!(!hit(&w).post.mine.liked);
    let stale = hit(&w);
    // like: the index follows
    w.click("like:3");
    wait_for(&mut w, |w| hit(w).post.mine.liked);
    assert_eq!(hit(&w).post.like_count, 1);
    // boost, and unlike
    w.click("boost:3");
    wait_for(&mut w, |w| hit(w).post.mine.boosted);
    w.click("like:3");
    wait_for(&mut w, |w| !hit(w).post.mine.liked);
    assert_eq!((hit(&w).post.like_count, hit(&w).post.mine.boosted, hit(&w).post.boost_count), (0, true, 1));
    // unboost
    w.click("boost:3");
    wait_for(&mut w, |w| !hit(w).post.mine.boosted);
    // the post is gone from the feed, and the hit the sidebar still shows is from before the
    // reactions: opening it shows the post as the index has it now
    w.click("like:3");
    wait_for(&mut w, |w| hit(w).post.mine.liked);
    w.app.social.timelines.get_mut(&key).unwrap().posts.retain(|q| q.id != p.id);
    w.app.open_post_hit(&stale);
    w.frames(4);
    let shown = w.app.social.timelines[&key].posts.iter().find(|q| q.id == p.id).cloned().expect("put into the list");
    assert_eq!((shown.mine.liked, shown.like_count), (true, 1), "fresh, not as the stale hit had it");
}

// spec: SOC-059
#[test]
fn soc_059_a_reaction_to_a_quoted_post_changes_it_inside_the_post_that_quotes_it_in_the_index() {
    let mut w = window("index-quote");
    let acct = home().account;
    let mut c = card(&w);
    c.text_markdown = "a kumquat".into();
    c.created_at = crate::calendar::time::now() - 3600;
    w.app.social.index.add(&acct, "home", std::slice::from_ref(&c));
    w.click("qlike:0");
    wait_for(&mut w, |w| w.app.social.index.search_wild("kumquat", 5)[0].post.quote.as_ref().unwrap().mine.liked);
    assert!(!w.app.social.index.search_wild("kumquat", 5)[0].post.mine.liked, "the post itself is not touched");
}

// spec: SOC-059
#[test]
fn soc_059_a_hit_is_shown_from_the_copy_in_memory_when_a_feed_holds_one() {
    let mut w = W::new("live-copy");
    let acct = home().account;
    let list = (acct.clone(), "list:1".to_string());
    let home_key = (acct.clone(), "home".to_string());
    // the index has the post from before the like, the other feed holds it as it is now
    let mut p = post("fresh1", "bob", crate::calendar::time::now() - 3600 * 5, None);
    p.text_markdown = "a medlar".into();
    w.app.social.index.add(&acct, "list:1", std::slice::from_ref(&p));
    let mut live = p.clone();
    live.mine.liked = true;
    live.like_count = 4;
    w.app.social.timelines.get_mut(&list).unwrap().posts.insert(20, live);
    let hit = w.app.social.index.search_wild("medlar", 5).remove(0);
    w.app.social.show_hit(1, &hit);
    assert!(w.app.social.take_focus(1, &home_key).is_some());
    let shown = w.app.social.timelines[&home_key].posts.iter().find(|q| q.id == "fresh1").cloned().expect("put into the list");
    assert_eq!((shown.mine.liked, shown.like_count), (true, 4));
}
