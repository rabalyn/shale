//! Writing from the Feeds tab (SOC-050 .. SOC-057): the like and boost buttons, the compose box with
//! its confirmation, pictures, quotes, and the Mastodon permission to write. The window runs
//! headless like in `ui_tests.rs`, the pretend server is `ui_tests::Script`. What goes over the
//! wire is checked against mock servers in `mastodon.rs` and `bluesky.rs`.

use super::actions::{ActionError, InstanceInfo, Reacted, Reaction, Visibility};
use super::compose::{Compose, Stage};
use super::tests::post;
use super::ui_tests::{home, wait_for, W};
use super::{thread_view_of, Network, Post};
use crate::app::Modal;
use eframe::egui::{Event, Key, Modifiers, Rect};

fn home_post(w: &W, id: &str) -> Post {
    let key = (home().account, "home".to_string());
    w.app.social.timelines[&key].posts.iter().find(|p| p.id == id).cloned().unwrap_or_else(|| panic!("no post {id}"))
}

fn toast_has(w: &W, text: &str) -> bool {
    w.app.toasts.iter().any(|t| t.0.contains(text))
}

/// Type into the compose box: the text field takes the keyboard.
fn type_text(w: &mut W, s: &str) {
    w.click("compose:text");
    w.frame(vec![Event::Text(s.into())]);
    w.frames(2);
}

fn ctrl_enter(w: &mut W) {
    w.frame(vec![Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
    w.frames(2);
}

fn size(r: Rect) -> (f32, f32) {
    (r.width().round(), r.height().round())
}

// spec: SOC-050
#[test]
fn soc_050_a_favourite_shows_a_spinner_in_its_button_keeps_its_size_and_changes_the_post_only_when_the_server_says_yes() {
    let mut w = W::new("like");
    let (tx, rx) = std::sync::mpsc::channel();
    *w.script.react_gate.lock().unwrap() = Some(rx);
    w.script.react_answers.lock().unwrap().push(Ok(Reacted { on: true, record: None, count: Some(7) }));
    let (like, save) = (w.rect("like:0"), w.rect("save:0"));
    assert!(!home_post(&w, "h0").mine.liked);
    w.click("like:0");
    // the job waits at the gate: a spinner in the button, the button keeps its size, nothing next to it moves
    w.frames(2);
    assert!(w.app.social.rects.iter().any(|r| r.0 == "like-spinner:0"), "the spinner shows");
    assert_eq!((size(w.rect("like:0")), w.rect("save:0")), (size(like), save), "nothing moves");
    assert_eq!(w.rect("like:0").min, like.min);
    // pressed again while it runs: no second request, and the post is not changed yet
    w.click("like:0");
    assert_eq!(w.script.reacts.lock().unwrap().len(), 1);
    let p = home_post(&w, "h0");
    assert!((p.mine.liked, p.like_count) == (false, 0), "nothing changes before the answer");
    tx.send(()).unwrap();
    wait_for(&mut w, |w| home_post(w, "h0").mine.liked);
    w.frames(3);
    assert_eq!(home_post(&w, "h0").like_count, 7, "the count of the server");
    assert!(!w.app.social.rects.iter().any(|r| r.0 == "like-spinner:0"));
    assert_eq!((size(w.rect("like:0")), w.rect("save:0")), (size(like), save), "the same size with and without the spinner");
    // pressing it again takes the favourite back
    w.script.react_answers.lock().unwrap().push(Ok(Reacted { on: false, record: None, count: Some(6) }));
    tx.send(()).unwrap();
    w.click("like:0");
    wait_for(&mut w, |w| !home_post(w, "h0").mine.liked);
    assert_eq!(home_post(&w, "h0").like_count, 6);
    assert_eq!(*w.script.reacts.lock().unwrap(), [("h0".to_string(), Reaction::Like, true), ("h0".to_string(), Reaction::Like, false)]);
}

// spec: SOC-050
#[test]
fn soc_050_a_boost_without_a_count_from_the_server_counts_one_more_and_one_less_and_asks_nothing() {
    let mut w = W::new("boost");
    w.click("boost:1");
    wait_for(&mut w, |w| home_post(w, "h1").mine.boosted);
    assert_eq!((home_post(&w, "h1").boost_count, home_post(&w, "h0").boost_count), (1, 0), "only the post that was boosted");
    assert!(w.app.social.compose.is_none() && w.app.modal.is_none(), "a boost does not ask first");
    w.click("boost:1");
    wait_for(&mut w, |w| !home_post(w, "h1").mine.boosted);
    assert_eq!(home_post(&w, "h1").boost_count, 0);
}

// spec: SOC-050
#[test]
fn soc_050_a_failed_reaction_changes_nothing_and_says_why() {
    let mut w = W::new("like-fails");
    w.script.react_answers.lock().unwrap().push(Err(ActionError::Failed("the server is down".into())));
    w.click("like:0");
    wait_for(&mut w, |w| w.app.social.writes.busy.is_empty() && toast_has(w, "Could not favourite the post: the server is down"));
    let p = home_post(&w, "h0");
    assert_eq!((p.mine.liked, p.like_count), (false, 0), "rolled back: it never changed");
    // and the button works again
    w.click("like:0");
    wait_for(&mut w, |w| home_post(w, "h0").mine.liked);
}

// spec: SOC-050
#[test]
fn soc_050_the_buttons_are_on_the_cards_of_the_conversation_too() {
    let mut w = W::new("like-thread");
    let key = (home().account, "home".to_string());
    let root = w.app.social.timelines[&key].posts[0].clone();
    let now = crate::calendar::time::now();
    let view = thread_view_of(&root, vec![], &[post("r1", "carol", now - 100, Some("h0"))]);
    w.script.threads.lock().unwrap().insert("h0".into(), vec![Ok(view)]);
    w.app.open_thread(home(), &root, false);
    w.frames(3);
    wait_for(&mut w, |w| w.app.social.rects.iter().any(|r| r.0 == "like:1"));
    for k in ["reply:0", "boost:0", "like:0", "quote:0", "reply:1"] {
        w.rect(k);
    }
    w.click("like:1");
    let liked = |w: &W| match &w.app.social.threads.map[&(home().account, "h0".to_string())].status {
        super::thread::ThreadStatus::Ready(v) => v.replies[0].post.mine.liked,
        _ => false,
    };
    wait_for(&mut w, liked);
    assert!(w.app.social.rects.iter().any(|r| r.0 == "like:1"));
}

// spec: SOC-056
#[test]
fn soc_056_an_account_that_signed_in_with_read_only_is_sent_to_its_sign_in_before_anything_is_sent() {
    let mut w = W::new("read-only");
    w.app.cfg.social_accounts[0].account.scopes = Some("read".into());
    for (key, what) in [("like:0", "like"), ("boost:0", "boost"), ("reply:0", "reply"), ("quote:0", "quote"), ("new-post", "new post")] {
        w.app.modal = None;
        w.app.toasts.clear();
        w.click(key);
        assert!(toast_has(&w, "Sign in again to allow writing"), "{what}");
        match &w.app.modal {
            Some(Modal::Social(d)) => assert_eq!(d.editing.as_ref().map(|a| a.id.clone()), Some(home().account), "{what}: the Edit sign-in of the account"),
            _ => panic!("{what}: the accounts dialog opens"),
        }
    }
    assert!(w.script.reacts.lock().unwrap().is_empty() && w.script.sends.lock().unwrap().is_empty(), "nothing was sent");
    assert!(w.app.social.compose.is_none());
}

// spec: SOC-056
#[test]
fn soc_056_a_refusal_of_the_server_is_remembered_and_opens_the_sign_in_too() {
    let mut w = W::new("refused");
    assert_eq!(w.app.cfg.social_accounts[0].account.scopes, None, "an account from before: not known");
    w.script.react_answers.lock().unwrap().push(Err(ActionError::NeedsWrite));
    w.click("like:0");
    wait_for(&mut w, |w| matches!(w.app.modal, Some(Modal::Social(_))));
    assert_eq!(w.app.cfg.social_accounts[0].account.scopes.as_deref(), Some("read"), "remembered: the next try does not ask the server");
    assert!(toast_has(&w, "Sign in again to allow writing"));
    assert!(!home_post(&w, "h0").mine.liked);
    // the account is not marked for Bluesky-style accounts: a token that can write is not asked again
    let mut a = w.app.cfg.social_accounts[0].account.clone();
    assert_eq!(a.may_write(), Some(false));
    a.scopes = Some("read write".into());
    assert_eq!(a.may_write(), Some(true));
    a.scopes = Some("read write:statuses".into());
    assert_eq!(a.may_write(), Some(true));
    a.scopes = Some("read follow".into());
    assert_eq!(a.may_write(), Some(false));
    a.scopes = None;
    assert_eq!(a.may_write(), None);
    a.network = super::Network::Bluesky;
    assert_eq!(a.may_write(), Some(true), "app passwords can write");
}

fn compose(w: &W) -> &Compose {
    w.app.social.compose.as_ref().expect("the compose box is open")
}

/// Send what is written: Ctrl+Enter asks, "Post" in the question sends.
fn send_it(w: &mut W) {
    ctrl_enter(w);
    assert_eq!(compose(w).stage, Stage::Confirm, "the question comes first");
    w.click("compose:confirm-post");
}

// spec: SOC-051
#[test]
fn soc_051_the_new_post_button_opens_the_box_with_its_fields_and_the_toolbar_does_not_overlap() {
    let mut w = W::new("new-post");
    let keys = ["account-combo", "feed-combo", "check-now", "feed-menu", "new-post", "accounts"];
    for pair in keys.windows(2) {
        assert!(w.rect(pair[0]).max.x <= w.rect(pair[1]).min.x + 0.5, "{} overlaps {}", pair[0], pair[1]);
    }
    assert!(w.app.social.compose.is_none());
    w.click("new-post");
    let tab = w.app.tabbar.tabs[0].id;
    assert_eq!((compose(&w).tab, compose(&w).account.clone(), compose(&w).feed.as_str(), compose(&w).text.as_str()), (tab, home().account, "home", ""));
    for k in ["compose:text", "compose:cw-toggle", "compose:visibility", "compose:counter", "compose:path", "compose:add-picture", "compose:send", "compose:cancel"] {
        w.rect(k);
    }
    type_text(&mut w, "Hello world");
    assert_eq!(compose(&w).text, "Hello world");
    // the content warning field opens with its box
    w.click("compose:cw-toggle");
    assert!(compose(&w).cw_on);
    w.rect("compose:cw");
    // Cancel closes without sending
    w.click("compose:cancel");
    assert!(w.app.social.compose.is_none() && w.script.sends.lock().unwrap().is_empty());
    // a box with something written is kept when another one is asked for; Esc closes it
    w.click("new-post");
    type_text(&mut w, "kept");
    w.click("reply:0");
    assert!(toast_has(&w, "Finish or cancel the post you are writing first"));
    assert_eq!((compose(&w).text.as_str(), compose(&w).reply.is_none()), ("kept", true));
    w.frame(vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    w.frames(2);
    assert!(w.app.social.compose.is_none());
}

// spec: SOC-055
#[test]
fn soc_055_send_asks_post_this_first_cancel_goes_back_to_the_text_and_post_sends() {
    let mut w = W::new("confirm");
    w.click("new-post");
    // nothing to send yet: the button does nothing
    w.click("compose:send");
    assert_eq!(compose(&w).stage, Stage::Edit);
    type_text(&mut w, "Hello world");
    w.click("compose:send");
    assert_eq!(compose(&w).stage, Stage::Confirm, "Send asks first");
    w.rect("compose:confirm-who");
    assert!(w.script.sends.lock().unwrap().is_empty());
    w.click("compose:confirm-cancel");
    assert_eq!((compose(&w).stage, compose(&w).text.as_str()), (Stage::Edit, "Hello world"), "Cancel goes back to the text, which is kept");
    // Ctrl+Enter is Send, and still asks; Esc goes back, and once more closes
    ctrl_enter(&mut w);
    assert_eq!(compose(&w).stage, Stage::Confirm);
    w.frame(vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    w.frames(2);
    assert_eq!(compose(&w).stage, Stage::Edit);
    assert!(w.script.sends.lock().unwrap().is_empty(), "nothing was sent so far");
    ctrl_enter(&mut w);
    w.click("compose:confirm-post");
    wait_for(&mut w, |w| w.app.social.compose.is_none());
    let sent = w.script.sends.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!((sent[0].text.as_str(), sent[0].visibility, sent[0].reply_to.is_none(), sent[0].quote.is_none(), sent[0].cw.as_str()), ("Hello world", Visibility::Public, true, true, ""));
}

// spec: SOC-055
#[test]
fn soc_055_while_it_is_sent_the_box_is_busy_and_nothing_goes_out_twice() {
    let mut w = W::new("sending");
    let (tx, rx) = std::sync::mpsc::channel();
    *w.script.send_gate.lock().unwrap() = Some(rx);
    w.click("new-post");
    type_text(&mut w, "Once");
    send_it(&mut w);
    w.frames(3);
    assert_eq!(compose(&w).stage, Stage::Sending);
    w.click("compose:send");
    ctrl_enter(&mut w);
    w.click("compose:cancel");
    assert!(w.app.social.compose.is_some(), "Cancel does not work while it is sent");
    assert_eq!(w.script.sends.lock().unwrap().len(), 1);
    tx.send(()).unwrap();
    wait_for(&mut w, |w| w.app.social.compose.is_none());
    assert_eq!(w.script.sends.lock().unwrap().len(), 1);
}

// spec: SOC-057
#[test]
fn soc_057_a_sent_post_waits_above_the_list_without_moving_the_view_and_is_indexed_and_a_failure_keeps_the_text() {
    let mut w = W::new("after-send");
    let acct = home().account;
    let top = w.rect("save:0");
    w.click("new-post");
    type_text(&mut w, "Hello again");
    w.script.send_answers.lock().unwrap().push(Err(ActionError::Failed("the server said no".into())));
    send_it(&mut w);
    wait_for(&mut w, |w| w.app.social.compose.as_ref().is_some_and(|c| c.stage == Stage::Edit && c.error.is_some()));
    assert_eq!((compose(&w).text.as_str(), compose(&w).error.as_deref()), ("Hello again", Some("the server said no")), "a failed send keeps the text and says why");
    w.rect("compose:error");
    assert_eq!(w.app.social.waiting_count(&acct, "home"), 0);
    // the second try works: the box closes, the post waits (SOC-027), the list did not move
    send_it(&mut w);
    wait_for(&mut w, |w| w.app.social.compose.is_none());
    w.frames(3);
    assert_eq!(w.app.social.waiting_count(&acct, "home"), 1);
    assert!(toast_has(&w, "Posted"));
    assert_eq!(w.rect("save:0"), top, "the view stays where it was");
    w.rect("new-posts");
    assert!(w.app.social.index.contains(&acct, "new-post-1|"), "the post is in the post index");
    assert_eq!(w.title(0), "Mastodon · Home (1)");
    // a button shows it above the list
    w.click("new-posts");
    assert_eq!(home_post(&w, "new-post-1").text_markdown, "Hello again");
}

// spec: SOC-056
#[test]
fn soc_056_a_send_that_the_server_refuses_for_its_permissions_keeps_the_text_and_opens_the_sign_in() {
    let mut w = W::new("send-refused");
    w.click("new-post");
    type_text(&mut w, "Hello");
    w.script.send_answers.lock().unwrap().push(Err(ActionError::NeedsWrite));
    send_it(&mut w);
    wait_for(&mut w, |w| matches!(w.app.modal, Some(Modal::Social(_))));
    assert_eq!(compose(&w).error.as_deref(), Some("Sign in again to allow writing"));
    assert_eq!((compose(&w).stage, compose(&w).text.as_str()), (Stage::Edit, "Hello"));
    assert_eq!(w.app.cfg.social_accounts[0].account.scopes.as_deref(), Some("read"));
}

// spec: SOC-051
#[test]
fn soc_051_a_reply_names_the_post_it_answers_starts_with_the_mention_and_counts_on_the_post() {
    let mut w = W::new("reply");
    let key = (home().account, "home".to_string());
    w.app.social.timelines.get_mut(&key).unwrap().posts[0].visibility = Some("private".into());
    w.click("reply:0");
    assert_eq!((compose(&w).reply.as_ref().map(|p| p.id.clone()), compose(&w).text.as_str(), compose(&w).visibility), (Some("h0".into()), "@bob@m.test ", Visibility::Private), "mention of the author, no more public than the post");
    type_text(&mut w, "thanks");
    assert!(compose(&w).text.contains("thanks"));
    send_it(&mut w);
    wait_for(&mut w, |w| w.app.social.compose.is_none());
    let sent = w.script.sends.lock().unwrap()[0].clone();
    assert_eq!((sent.reply_to.as_ref().map(|p| p.id.as_str()), sent.visibility), (Some("h0"), Visibility::Private));
    assert!(sent.text.starts_with("@bob@m.test "));
    assert_eq!(home_post(&w, "h0").reply_count, 1, "the post counts the reply");
    // a reply to a post of the account itself needs no mention, and a public post gives the last visibility
    w.app.social.timelines.get_mut(&key).unwrap().posts[1].author.handle = "ada@demo.social".into();
    w.click("reply:1");
    assert_eq!((compose(&w).text.as_str(), compose(&w).visibility), ("", Visibility::Private), "the visibility of the last post is kept");
}

// spec: SOC-057
#[test]
fn soc_057_a_reply_sent_from_the_conversation_joins_it() {
    let mut w = W::new("reply-thread");
    let key = (home().account, "home".to_string());
    let root = w.app.social.timelines[&key].posts[0].clone();
    let view = thread_view_of(&root, vec![], &[post("r1", "carol", crate::calendar::time::now() - 100, Some("h0"))]);
    w.script.threads.lock().unwrap().insert("h0".into(), vec![Ok(view)]);
    w.app.open_thread(home(), &root, false);
    wait_for(&mut w, |w| w.app.social.rects.iter().any(|r| r.0 == "like:1"));
    w.click("reply:0");
    assert_eq!(compose(&w).reply.as_ref().map(|p| p.id.as_str()), Some("h0"));
    type_text(&mut w, "from the conversation");
    send_it(&mut w);
    wait_for(&mut w, |w| w.app.social.compose.is_none());
    w.frames(3);
    let ids = |w: &W| match &w.app.social.threads.map[&(home().account, "h0".to_string())].status {
        super::thread::ThreadStatus::Ready(v) => (v.post.reply_count, v.replies.iter().map(|r| r.post.id.clone()).collect::<Vec<_>>()),
        _ => panic!("the conversation is gone"),
    };
    assert_eq!(ids(&w), (1, vec!["r1".to_string(), "new-post-1".to_string()]), "the reply is in the tree, the opened post counts it");
    assert!(w.app.social.rects.iter().any(|r| r.0 == "like:2"), "and it has its card");
}

// spec: SOC-052
#[test]
fn soc_052_the_counter_follows_the_limit_of_the_server_and_send_waits_until_the_text_fits() {
    let mut w = W::new("limit");
    *w.script.info.lock().unwrap() = Some(InstanceInfo { max_chars: 20, ..InstanceInfo::defaults(Network::Mastodon) });
    w.click("new-post");
    let acct = home().account;
    wait_for(&mut w, |w| w.app.social.writes.info.contains_key(&acct));
    type_text(&mut w, "0123456789012345678901234");
    let info = w.app.social.info_for(&w.app.cfg.social_accounts[0].account);
    assert_eq!((info.max_chars, compose(&w).length(Network::Mastodon, &info)), (20, 25));
    assert_eq!(compose(&w).problem(Network::Mastodon, &info), Some("The post is too long"));
    // too long: neither the button nor Ctrl+Enter go on
    w.click("compose:send");
    ctrl_enter(&mut w);
    assert_eq!(compose(&w).stage, Stage::Edit);
    w.app.social.compose.as_mut().unwrap().text = "01234567890123456789".into();
    w.frames(2);
    ctrl_enter(&mut w);
    assert_eq!(compose(&w).stage, Stage::Confirm, "exactly at the limit is fine");
}

// spec: SOC-053
#[test]
fn soc_053_pictures_are_added_by_path_get_a_description_and_can_be_removed_and_a_picture_alone_is_a_post() {
    let mut w = W::new("pictures");
    let dir = w.dir.clone();
    for n in ["a.png", "b.png", "c.png", "d.png", "e.png"] {
        std::fs::write(dir.join(n), crate::social::actions::tests::png(20, 20, false)).unwrap();
    }
    std::fs::write(dir.join("words.png"), "no picture").unwrap();
    w.click("new-post");
    assert!(w.app.social.compose.as_ref().unwrap().problem(Network::Mastodon, &InstanceInfo::defaults(Network::Mastodon)).is_some(), "an empty post cannot be sent");
    let add = |w: &mut W, path: &str| {
        w.app.social.compose.as_mut().unwrap().path = path.into();
        w.frames(2);
        w.click("compose:add-picture");
    };
    // a file of the vault by its path inside it, a missing file and a text file
    add(&mut w, "a.png");
    assert_eq!((compose(&w).pictures.len(), compose(&w).pictures[0].path.clone(), compose(&w).path.as_str()), (1, dir.join("a.png"), ""));
    add(&mut w, "gone.png");
    assert!(compose(&w).error.as_deref().is_some_and(|e| e.contains("cannot be read")), "{:?}", compose(&w).error);
    add(&mut w, "words.png");
    assert!(compose(&w).error.as_deref().is_some_and(|e| e.contains("is not a JPEG, PNG, GIF or WebP")));
    add(&mut w, "a.png");
    assert_eq!(compose(&w).error.as_deref(), Some("This picture is in the post already."));
    // a picture alone is a post; its description is typed into its own field
    w.click("compose:alt:0");
    w.frame(vec![Event::Text("A gradient".into())]);
    w.frames(2);
    assert_eq!(compose(&w).pictures[0].alt, "A gradient");
    // at most four
    for n in ["b.png", "c.png", "d.png"] {
        add(&mut w, n);
    }
    assert_eq!(compose(&w).pictures.len(), 4);
    assert!(!w.app.social.rects.iter().any(|r| r.0 == "compose:path"), "no field for a fifth picture");
    w.click("compose:remove:3");
    assert_eq!(compose(&w).pictures.len(), 3);
    send_it(&mut w);
    wait_for(&mut w, |w| w.app.social.compose.is_none());
    let sent = w.script.sends.lock().unwrap()[0].clone();
    assert_eq!((sent.text.as_str(), sent.pictures.len(), sent.pictures[0].alt.as_str(), sent.pictures[0].path.clone()), ("", 3, "A gradient", dir.join("a.png")));
}

// spec: SOC-054
#[test]
fn soc_054_a_quote_without_server_support_puts_the_address_into_the_text_and_says_so() {
    let mut w = W::new("quote-fallback");
    let key = (home().account, "home".to_string());
    let url = w.app.social.timelines[&key].posts[1].url.clone();
    w.click("quote:1");
    assert_eq!(compose(&w).quote.as_ref().map(|q| q.id.as_str()), Some("h1"));
    w.rect("compose:quote-note");
    type_text(&mut w, "Look");
    let info = w.app.social.info_for(&w.app.cfg.social_accounts[0].account);
    assert_eq!(compose(&w).length(Network::Mastodon, &info), 4 + 2 + 23, "the address counts as one");
    send_it(&mut w);
    wait_for(&mut w, |w| w.app.social.compose.is_none());
    let sent = w.script.sends.lock().unwrap()[0].clone();
    assert_eq!((sent.text, sent.quote.is_none()), (format!("Look\n\n{url}"), true));
}

// spec: SOC-054
#[test]
fn soc_054_a_server_that_can_quote_gets_the_quoted_post_and_a_text_without_the_address() {
    let mut w = W::new("quote-real");
    *w.script.info.lock().unwrap() = Some(InstanceInfo { quotes: true, ..InstanceInfo::defaults(Network::Mastodon) });
    let acct = home().account;
    w.click("quote:1");
    wait_for(&mut w, |w| w.app.social.writes.info.get(&acct).is_some_and(|i| i.quotes));
    w.frames(2);
    assert!(!w.app.social.rects.iter().any(|r| r.0 == "compose:quote-note"), "no note: it is a real quote");
    type_text(&mut w, "Look");
    send_it(&mut w);
    wait_for(&mut w, |w| w.app.social.compose.is_none());
    let sent = w.script.sends.lock().unwrap()[0].clone();
    assert_eq!((sent.text.as_str(), sent.quote.map(|q| q.id)), ("Look", Some("h1".to_string())));
}

// spec: SOC-052
#[test]
fn soc_052_a_bluesky_box_has_no_content_warning_and_no_visibility_and_a_limit_of_300() {
    let mut w = W::new("bsky-box");
    w.app.cfg.social_accounts[0].account.network = Network::Bluesky;
    w.click("new-post");
    w.rect("compose:no-cw");
    assert!(!w.app.social.rects.iter().any(|r| r.0 == "compose:cw-toggle" || r.0 == "compose:visibility"));
    let info = w.app.social.info_for(&w.app.cfg.social_accounts[0].account);
    assert_eq!(info.max_chars, 300);
    // even a warning typed before is not sent
    w.app.social.compose.as_mut().unwrap().cw_on = true;
    w.app.social.compose.as_mut().unwrap().cw = "spoiler".into();
    type_text(&mut w, "Hello");
    send_it(&mut w);
    wait_for(&mut w, |w| w.app.social.compose.is_none());
    assert_eq!(w.script.sends.lock().unwrap()[0].cw, "");
}

// spec: SOC-052
#[test]
fn soc_052_each_network_counts_in_its_own_way() {
    let m = InstanceInfo::defaults(Network::Mastodon);
    let b = InstanceInfo::defaults(Network::Bluesky);
    assert_eq!((m.max_chars, b.max_chars), (500, 300));
    let with = |text: &str| Compose { text: text.into(), ..Compose::new(1, "a", "home") };
    // Mastodon: an address counts 23, a mention only its user name, the words and spaces by characters
    assert_eq!(with("hi https://example.com/a/very/long/path?x=1 ok").length(Network::Mastodon, &m), 3 + 23 + 3);
    assert_eq!(with("see https://e.com.").length(Network::Mastodon, &m), 4 + 23 + 1);
    assert_eq!(with("@bob@m.test hi").length(Network::Mastodon, &m), 4 + 3);
    assert_eq!(with("héllo 😀").length(Network::Mastodon, &m), 7);
    // the content warning counts as well
    let cw = Compose { cw_on: true, cw: "spoiler".into(), ..with("abc") };
    assert_eq!(cw.length(Network::Mastodon, &m), 3 + 7);
    // Bluesky counts graphemes, an address as it is
    assert_eq!(with("a😀e\u{301}👨‍👩‍👧 https://e.com/x").length(Network::Bluesky, &b), 4 + 1 + 15);
    assert!(with(&"x".repeat(301)).problem(Network::Bluesky, &b).is_some() && with(&"x".repeat(300)).problem(Network::Bluesky, &b).is_none());
    assert_eq!(with("  \n").problem(Network::Bluesky, &b), Some("Write something or add a picture"));
}
