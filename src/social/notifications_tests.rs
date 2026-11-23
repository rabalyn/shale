//! Notifications as a feed and "Your posts" (SOC-068 .. SOC-075): what goes over the wire against
//! mock servers (Mastodon and Bluesky, through real `curl`), and the feed in the window with the
//! pretend server of `ui_tests::Script`.

use super::actions::{Reaction, Sent};
use super::notifications::{entries, newest, Kind, Raw, NOTIFICATIONS, OWN};
use super::state::{post_key, Timeline};
use super::tests::post;
use super::ui_tests::{home, W};
use super::{demo, Account, Author, Client, Feed, FeedRef, Network, Page, Post, Since};
use crate::app::View;
use crate::calendar::dav::tests::mock_http;
use serde_json::{json, Value};

fn person(id: &str) -> Author {
    Author { id: id.into(), handle: format!("{id}@m.test"), display_name: id.to_uppercase(), profile_url: format!("https://m.test/@{id}"), avatar_url: String::new() }
}

fn raw(id: i64, kind: Kind, who: &str, post: Option<Post>, unread: bool) -> Raw {
    Raw { id: id.to_string(), mark: id.to_string(), rank: id, kind, who: person(who), at: 1_000_000 + id, unread, post }
}

// ---------------------------------------------------------------- Mastodon

fn status(id: &str, acct: &str, text: &str, reply_to: Option<&str>) -> Value {
    json!({"id": id, "uri": format!("https://m.test/users/{acct}/statuses/{id}"), "url": format!("https://m.test/@{acct}/{id}"), "created_at": "2026-11-01T10:00:00.000Z", "in_reply_to_id": reply_to, "spoiler_text": "",
           "content": format!("<p>{text}</p>"), "account": {"id": format!("id-{acct}"), "acct": acct, "display_name": acct.to_uppercase(), "url": format!("https://m.test/@{acct}"), "avatar": ""},
           "media_attachments": [], "replies_count": 0, "reblogs_count": 0, "favourites_count": 0})
}

fn note(id: &str, kind: &str, acct: &str, status: Option<Value>) -> Value {
    json!({"id": id, "type": kind, "created_at": "2026-11-01T10:00:00.000Z", "account": {"id": format!("id-{acct}"), "acct": acct, "display_name": acct.to_uppercase(), "url": format!("https://m.test/@{acct}"), "avatar": ""}, "status": status})
}

/// The notifications of the Mastodon tests, newest first.
fn mastodon_items() -> Value {
    json!([
        note("910", "favourite", "bob", Some(status("500", "ada", "mine", None))),
        note("909", "favourite", "cy", Some(status("500", "ada", "mine", None))),
        note("908", "mention", "dee", Some(status("600", "dee", "hello @ada", None))),
        note("907", "mention", "fay", Some(status("601", "fay", "an answer", Some("500")))),
        note("906", "follow", "eve", None),
        note("905", "admin.sign_up", "gus", None),
        note("904", "reblog", "fay", Some(status("501", "ada", "second", None))),
        note("890", "poll", "ada", Some(status("502", "ada", "poll", None))),
    ])
}

// spec: SOC-069, SOC-071
#[test]
fn soc_069_mastodon_notifications_become_entries_with_their_kind_unread_state_and_grouped_likes() {
    let posts = super::notifications::mastodon_entries(mastodon_items().as_array().unwrap(), "https://m.test", 905);
    let shape: Vec<(Kind, String, bool, String)> = posts.iter().map(|p| p.notice.as_ref().map(|n| (n.kind, n.who_text(), n.unread, p.id.clone())).unwrap()).collect();
    assert_eq!(
        shape,
        [
            (Kind::Like, "BOB and CY".into(), true, "500".into()),
            (Kind::Mention, "DEE".into(), true, "600".into()),
            (Kind::Reply, "FAY".into(), true, "601".into()),
            (Kind::Follow, "EVE".into(), true, "notif:906".into()),
            (Kind::Boost, "FAY".into(), false, "501".into()),
            (Kind::Poll, "ADA".into(), false, "502".into()),
        ],
        "likes of one post are one entry, a mention that answers a post is a reply, what is about the server is left out"
    );
    let like = posts[0].notice.as_ref().unwrap();
    assert_eq!((like.id.as_str(), like.mark.as_str(), like.rank, like.ids.clone()), ("910", "910", 910, vec!["910".to_string(), "909".to_string()]));
    assert_eq!(like.line(Network::Mastodon), "BOB and CY favourited your post");
    assert_eq!(posts[3].author.handle, "eve@m.test", "a follow is the person");
    assert_eq!(posts[3].url, "https://m.test/@eve");
    assert_eq!(newest(&posts).unwrap().mark, "910");
    // every notification is its own entry in the list, also for the same post
    assert_ne!(post_key(&posts[0]), post_key(&Post { notice: None, ..posts[0].clone() }));
    let again = super::notifications::mastodon_entries(&mastodon_items().as_array().unwrap()[1..2], "https://m.test", 0);
    assert_eq!(again.len(), 1);
    assert_ne!(post_key(&again[0]), post_key(&posts[0]), "another notification about the post is another entry");
}

// spec: SOC-068, SOC-069, SOC-072, SOC-074
#[test]
fn soc_069_mastodon_notifications_page_check_and_mark_read_and_your_posts_ask_for_the_account_against_a_mock_server() {
    if std::process::Command::new("curl").arg("--version").output().is_err() {
        return;
    }
    let items = mastodon_items();
    let newer = json!([note("912", "favourite", "hal", Some(status("500", "ada", "mine", None)))]);
    let (base, handle) = mock_http(13, move |method, path, headers, body| {
        let authed = headers.to_ascii_lowercase().contains("authorization: bearer tok-1");
        assert!(authed, "{method} {path}: {headers}");
        let ok = |v: Value| (200, v.to_string());
        match (method, path) {
            ("GET", "/api/v1/lists") => ok(json!([{"id": "7", "title": "Friends"}])),
            ("GET", "/api/v1/markers?timeline%5B%5D=notifications") => ok(json!({"notifications": {"last_read_id": "905", "version": 3, "updated_at": "2026-11-01T10:00:00.000Z"}})),
            ("GET", "/api/v1/notifications?limit=20") => ok(items.clone()),
            ("GET", "/api/v1/notifications?limit=20&max_id=890") => ok(json!([])),
            ("GET", "/api/v1/notifications?limit=40&min_id=910") => ok(newer.clone()),
            ("GET", "/api/v1/notifications?limit=40&min_id=912") => ok(json!([])),
            ("POST", "/api/v1/markers") => {
                assert_eq!(serde_json::from_str::<Value>(body).unwrap(), json!({"notifications": {"last_read_id": "912"}}));
                ok(json!({"notifications": {"last_read_id": "912"}}))
            }
            ("GET", "/api/v1/accounts/verify_credentials") => ok(json!({"id": "55", "acct": "ada"})),
            ("GET", "/api/v1/accounts/55/statuses?limit=20") => ok(json!([status("700", "ada", "my post", None)])),
            ("GET", "/api/v1/accounts/55/statuses?limit=20&max_id=700") => ok(json!([])),
            ("GET", "/api/v1/accounts/55/statuses?limit=40&min_id=700") => ok(json!([])),
            _ => (404, json!({"error": "not found"}).to_string()),
        }
    });
    let mut c = Client::new(Account::new(Network::Mastodon, &base, "ada@m.test", "Ada"), "tok-1");
    let feeds = c.feeds().unwrap();
    assert_eq!(feeds.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["home", NOTIFICATIONS, OWN, "list:7"], "SOC-068");
    assert_eq!(feeds.iter().map(|f| f.title.as_str()).collect::<Vec<_>>(), ["Home", "Notifications", "Your posts", "Friends"]);
    let notifications = &feeds[1];
    // the first page: entries, the cursor is the last notification (also one that is left out), the top the newest
    let page = c.timeline(notifications, None).unwrap();
    assert_eq!((page.posts.len(), page.cursor.as_deref(), page.top.as_deref()), (6, Some("890"), Some("910")));
    assert!(page.posts[0].notice.as_ref().unwrap().unread && !page.posts[4].notice.as_ref().unwrap().unread, "unread by the marker of the server");
    let next = c.timeline(notifications, page.cursor.as_deref()).unwrap();
    assert!(next.posts.is_empty() && next.cursor.is_none());
    // a check asks for what is above the newest notification, page by page
    let fresh = c.newer(notifications, &Since { top: page.top.clone(), ..Default::default() }).unwrap();
    assert_eq!((fresh.posts.len(), fresh.top.as_deref()), (1, Some("912")));
    assert_eq!(fresh.posts[0].notice.as_ref().unwrap().kind, Kind::Like);
    c.mark_notifications("912").unwrap();
    // "Your posts": the id of the account first, then its statuses
    let own = c.timeline(&feeds[2], None).unwrap();
    assert_eq!(own.posts.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["700"]);
    // the id is kept: a second page and a check do not ask again (13 requests in all, one verify_credentials)
    assert!(c.timeline(&feeds[2], Some("700")).unwrap().posts.is_empty());
    assert!(own.posts[0].notice.is_none());
    assert!(c.newer(&feeds[2], &Since { top: own.top.clone(), ..Default::default() }).unwrap().posts.is_empty());
    let seen = handle.join().unwrap();
    assert_eq!(seen.len(), 13, "{seen:?}");
    assert_eq!(seen.iter().filter(|r| r.contains("verify_credentials")).count(), 1, "{seen:?}");
}

// spec: SOC-072
#[test]
fn soc_072_a_mark_that_is_no_notification_id_is_refused_before_a_request_is_made() {
    assert!(super::notifications::mastodon_mark("https://m.test", "tok", "").is_err());
    assert!(super::notifications::mastodon_mark("https://m.test", "tok", "12/../x").is_err());
    let mut api = super::bluesky::Api::new("https://bsky.test", "ada.test", "pw");
    assert!(super::notifications::bluesky_mark(&mut api, "").is_err());
}

// ---------------------------------------------------------------- Bluesky

fn bsky_post(n: &str, who: &str, text: &str, reply_to: Option<&str>) -> Value {
    let did = format!("did:plc:{who}");
    let mut record = json!({"$type": "app.bsky.feed.post", "text": text, "createdAt": "2026-11-01T10:00:00.000Z"});
    if let Some(r) = reply_to {
        record["reply"] = json!({"parent": {"uri": r, "cid": "c"}, "root": {"uri": r, "cid": "c"}});
    }
    json!({"uri": format!("at://{did}/app.bsky.feed.post/{n}"), "cid": format!("cid{n}"), "author": {"did": did, "handle": format!("{who}.test"), "displayName": who.to_uppercase()}, "record": record,
           "replyCount": 0, "repostCount": 0, "likeCount": 4, "indexedAt": "2026-11-01T10:00:00.000Z", "viewer": {}})
}

fn bsky_note(uri: &str, reason: &str, who: &str, subject: Option<&str>, at: &str, read: bool) -> Value {
    let mut v = json!({"uri": uri, "cid": "c", "author": {"did": format!("did:plc:{who}"), "handle": format!("{who}.test"), "displayName": who.to_uppercase()}, "reason": reason, "record": {}, "isRead": read, "indexedAt": at});
    if let Some(s) = subject {
        v["reasonSubject"] = json!(s);
    }
    v
}

// spec: SOC-068, SOC-070, SOC-071, SOC-072, SOC-074
#[test]
fn soc_070_bluesky_notifications_fetch_their_posts_in_batches_stop_a_check_at_a_known_notification_and_mark_seen_against_a_mock_server() {
    if std::process::Command::new("curl").arg("--version").output().is_err() {
        return;
    }
    let mine = "at://did:plc:ada/app.bsky.feed.post/p1";
    let reply_uri = "at://did:plc:dee/app.bsky.feed.post/r1";
    let (l0, l1, l2) = ("at://did:plc:fay/app.bsky.feed.like/l0", "at://did:plc:bob/app.bsky.feed.like/l1", "at://did:plc:cy/app.bsky.feed.like/l2");
    let first = json!({"notifications": [
        bsky_note(l1, "like", "bob", Some(mine), "2026-11-01T10:05:00.000Z", false),
        bsky_note(l2, "like", "cy", Some(mine), "2026-11-01T10:04:00.000Z", false),
        bsky_note(reply_uri, "reply", "dee", Some(mine), "2026-11-01T10:03:00.000Z", true),
        bsky_note("at://did:plc:eve/app.bsky.graph.follow/f1", "follow", "eve", None, "2026-11-01T10:02:00.000Z", true),
        bsky_note("at://did:plc:gus/app.bsky.feed.like/l9", "like", "gus", Some("at://did:plc:ada/app.bsky.feed.post/gone"), "2026-11-01T10:01:30.000Z", true),
        bsky_note("at://did:plc:hal/app.bsky.graph.verification/v1", "verified", "hal", None, "2026-11-01T10:01:00.000Z", true),
    ], "cursor": "c1"});
    let check = json!({"notifications": [
        bsky_note(l0, "like", "fay", Some(mine), "2026-11-01T10:09:00.000Z", false),
        bsky_note(l1, "like", "bob", Some(mine), "2026-11-01T10:05:00.000Z", false),
    ]});
    let asked = std::sync::atomic::AtomicUsize::new(0);
    let (base, handle) = mock_http(8, move |method, path, headers, body| {
        let host = headers.lines().find_map(|l| l.strip_prefix("Host: ")).unwrap_or("").trim().to_string();
        let ok = |v: Value| (200, v.to_string());
        match (method, path.split('?').next().unwrap()) {
            ("POST", "/xrpc/com.atproto.server.createSession") => {
                ok(json!({"accessJwt": "jwt-1", "refreshJwt": "ref-1", "did": "did:plc:ada", "handle": "ada.test", "didDoc": {"service": [{"id": "#atproto_pds", "serviceEndpoint": format!("http://{host}")}]}}))
            }
            ("GET", "/xrpc/app.bsky.actor.getPreferences") => ok(json!({"preferences": []})),
            // the first asking is the page, the second the check
            ("GET", "/xrpc/app.bsky.notification.listNotifications") => {
                assert!(path.contains("limit=30") && !path.contains("cursor"), "{path}");
                ok(if asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 { first.clone() } else { check.clone() })
            }
            ("GET", "/xrpc/app.bsky.feed.getPosts") => {
                let mut posts = vec![];
                if path.contains("post%2Fp1") {
                    posts.push(bsky_post("p1", "ada", "plain files", None));
                }
                if path.contains("post%2Fr1") {
                    posts.push(bsky_post("r1", "dee", "which editor?", Some(mine)));
                }
                ok(json!({"posts": posts}))
            }
            ("POST", "/xrpc/app.bsky.notification.updateSeen") => {
                assert_eq!(serde_json::from_str::<Value>(body).unwrap(), json!({"seenAt": "2026-11-01T10:09:00.000Z"}));
                ok(json!({}))
            }
            ("GET", "/xrpc/app.bsky.feed.getAuthorFeed") => {
                assert!(path.contains("actor=did%3Aplc%3Aada") && path.contains("filter=posts_with_replies"), "{path}");
                ok(
                    json!({"feed": [{"post": bsky_post("p1", "ada", "plain files", None)}, {"post": bsky_post("p2", "cy", "boosted", None), "reason": {"$type": "app.bsky.feed.defs#reasonRepost", "by": {"did": "did:plc:ada", "handle": "ada.test"}}}], "cursor": "o1"}),
                )
            }
            _ => (404, json!({"error": "NotFound"}).to_string()),
        }
    });
    let mut c = Client::new(Account::new(Network::Bluesky, &base, "ada.test", "Ada"), "app-pw");
    let feeds = c.feeds().unwrap();
    assert_eq!(feeds.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["timeline", NOTIFICATIONS, OWN], "SOC-068");
    let page = c.timeline(&feeds[1], None).unwrap();
    let shape: Vec<(Kind, String, bool, String)> = page.posts.iter().map(|p| p.notice.as_ref().map(|n| (n.kind, n.who_text(), n.unread, p.id.clone())).unwrap()).collect();
    assert_eq!(
        shape,
        [(Kind::Like, "BOB and CY".into(), true, mine.into()), (Kind::Reply, "DEE".into(), false, reply_uri.into()), (Kind::Follow, "EVE".into(), false, "notif:at://did:plc:eve/app.bsky.graph.follow/f1".into()),],
        "a like of a post that is gone and a kind this version does not know are left out"
    );
    assert_eq!(page.cursor.as_deref(), Some("c1"));
    assert_eq!(page.posts[0].notice.as_ref().unwrap().line(Network::Bluesky), "BOB and CY liked your post");
    assert_eq!(page.posts[0].notice.as_ref().unwrap().ids, [l1, l2]);
    // a check stops at the first notification that an entry of the list covers, so the like that
    // is part of the entry "BOB and CY" does not make a second entry
    let known: std::collections::HashSet<String> = page.posts.iter().flat_map(|p| std::iter::once(post_key(p)).chain(p.notice.iter().flat_map(|n| n.ids.clone()))).collect();
    let fresh = c.newer(&feeds[1], &Since { top: None, known }).unwrap();
    assert_eq!(fresh.posts.iter().map(|p| p.notice.as_ref().unwrap().who_text()).collect::<Vec<_>>(), ["FAY"]);
    assert_eq!((fresh.gap, fresh.posts[0].id.as_str()), (None, mine));
    let newest = newest(&fresh.posts).unwrap();
    assert_eq!((newest.unread, newest.mark.as_str()), (true, "2026-11-01T10:09:00.000Z"));
    c.mark_notifications(&newest.mark).unwrap();
    // "Your posts": the posts, replies and reposts of the account (the DID is the one of the session)
    let own = c.timeline(&feeds[2], None).unwrap();
    assert_eq!(own.posts.len(), 2);
    assert_eq!(own.posts[1].reposted_by.as_ref().unwrap().handle, "ada.test");
    assert_eq!(own.cursor.as_deref(), Some("o1"));
    assert_eq!(handle.join().unwrap().len(), 8);
}

// ---------------------------------------------------------------- the entries

// spec: SOC-071
#[test]
fn soc_071_the_names_of_an_entry_and_what_groups() {
    let own = post("o1", "ada", 1, None);
    let other = post("o2", "ada", 2, None);
    let posts = entries(
        Network::Mastodon,
        vec![
            raw(9, Kind::Like, "a", Some(own.clone()), false),
            raw(8, Kind::Like, "b", Some(own.clone()), true),
            raw(7, Kind::Like, "c", Some(own.clone()), false),
            raw(6, Kind::Like, "d", Some(own.clone()), false),
            raw(5, Kind::Like, "a", Some(own.clone()), false),
            raw(4, Kind::Like, "e", Some(other.clone()), false),
            raw(3, Kind::Boost, "e", Some(other.clone()), false),
            raw(2, Kind::Follow, "f", None, false),
            raw(1, Kind::Follow, "g", None, false),
        ],
    );
    let who: Vec<String> = posts.iter().map(|p| p.notice.as_ref().unwrap().who_text()).collect();
    assert_eq!(who, ["A, B and 2 others", "E", "E", "F", "G"], "the same person twice counts once; another post, another kind and follows do not group");
    let first = posts[0].notice.as_ref().unwrap();
    assert!(first.unread, "one unread notification makes the entry unread");
    assert_eq!((first.id.as_str(), first.at, first.ids.len()), ("9", 1_000_009, 5));
    let one = |n: usize| {
        let mut p = entries(Network::Mastodon, vec![raw(1, Kind::Like, "a", Some(own.clone()), false)]).remove(0);
        let n = p.notice.as_deref_mut().map(|x| {
            x.who = (0..n).map(|i| person(&format!("p{i}"))).collect();
            x.who_text()
        });
        n.unwrap()
    };
    assert_eq!((one(2).as_str(), one(3).as_str()), ("P0 and P1", "P0, P1 and 1 other"));
    let verbs = |k: Kind, n: Network| entries(n, vec![raw(1, k, "a", Some(own.clone()), false)])[0].notice.as_ref().unwrap().line(n);
    assert_eq!(verbs(Kind::Like, Network::Bluesky), "A liked your post");
    assert_eq!(verbs(Kind::Boost, Network::Bluesky), "A reposted your post");
    assert_eq!(verbs(Kind::Boost, Network::Mastodon), "A boosted your post");
    assert_eq!(verbs(Kind::Poll, Network::Mastodon), "A's poll has ended");
}

// ---------------------------------------------------------------- the window

/// The entries of the window tests: a reply to a post of the account, a favourite, a follow.
fn feed_entries() -> Vec<Post> {
    let now = crate::calendar::time::now();
    let mut own = post("own1", "ada", now - 100, None);
    own.reply_count = 1;
    let mut reply = post("rep1", "bob", now - 200, Some("own1"));
    reply.reply_count = 1;
    entries(Network::Mastodon, vec![raw(30, Kind::Reply, "bob", Some(reply), true), raw(29, Kind::Like, "cy", Some(own), true), raw(28, Kind::Follow, "dee", None, false)])
}

fn notifications_ref() -> FeedRef {
    FeedRef::new(demo::account().account.id, NOTIFICATIONS)
}

/// A window on the Notifications feed that the pretend server answers with `feed_entries`.
fn on_notifications(tag: &str) -> W {
    on_page(tag, Page { posts: feed_entries(), top: Some("30".into()), ..Default::default() }, None)
}

/// A window on the Notifications feed whose first page is `page`; `next` is what the page after it
/// holds (the end of a short list is on screen, so the next page is asked for at once).
fn on_page(tag: &str, page: Page, next: Option<Page>) -> W {
    let mut w = W::new(tag);
    *w.script.first_page.lock().unwrap() = Some(page);
    *w.script.next_page.lock().unwrap() = next;
    w.click("feed-combo");
    w.click(&format!("feed:{NOTIFICATIONS}"));
    let key = (notifications_ref().account, NOTIFICATIONS.to_string());
    w.wait_for("the notifications did not load", |w| w.app.social.timelines.get(&key).is_some_and(|t| t.loaded));
    w.frames(3);
    w
}

// spec: SOC-068, SOC-071, SOC-072
#[test]
fn soc_071_the_notifications_feed_shows_a_row_and_the_post_as_a_card_and_reads_what_it_shows() {
    let mut w = on_notifications("notif-feed");
    assert_eq!(w.view(0), View::Social(Some(notifications_ref())));
    assert_eq!(w.title(0), "Mastodon · Notifications");
    // a row above each card; a dot for the unread ones only
    for i in 0..3 {
        assert!(w.rects_has(&format!("notice:{i}")), "row {i}");
    }
    assert!(w.rects_has("unread:0") && w.rects_has("unread:1") && !w.rects_has("unread:2"));
    // the post of a notification is a normal card with its buttons; a follow is the person and "Open in browser"
    for k in ["reply:0", "like:0", "boost:0", "quote:0", "save:0", "thread:0", "open:0", "like:1", "save:1"] {
        assert!(w.rects_has(k), "{k}");
    }
    assert!(w.rects_has("open:2") && !w.rects_has("save:2") && !w.rects_has("like:2"), "a follow has no post buttons");
    // the server is told: read up to the newest notification, once
    w.wait_for("the marker was not sent", |w| !w.script.marks.lock().unwrap().is_empty());
    w.frames(5);
    assert_eq!(*w.script.marks.lock().unwrap(), ["30"]);
    let key = (notifications_ref().account, NOTIFICATIONS.to_string());
    assert_eq!(w.app.social.timelines[&key].marked, 30);
    assert!(w.rects_has("unread:0"), "the dots stay while the list is on screen");
    // a card's buttons work as everywhere: a like goes to the post the notification is about
    w.click("like:1");
    w.wait_for("the favourite was not sent", |w| !w.script.reacts.lock().unwrap().is_empty());
    assert_eq!(*w.script.reacts.lock().unwrap(), [("own1".to_string(), Reaction::Like, true)]);
    // the conversation of the reply opens in this tab, as a step of its history
    w.click("thread:0");
    assert_eq!(w.view(0), View::Social(Some(FeedRef { thread: Some("rep1".into()), ..notifications_ref() })));
}

// spec: SOC-072, SOC-023, SOC-027
#[test]
fn soc_072_new_notifications_wait_behind_the_button_count_in_the_title_and_are_read_when_shown() {
    let mut w = on_notifications("notif-new");
    w.wait_for("the marker was not sent", |w| !w.script.marks.lock().unwrap().is_empty());
    // two new ones: one unread, one that the server already counts as read (read on another device)
    let mut newer = entries(Network::Mastodon, vec![raw(33, Kind::Like, "eve", Some(post("own2", "ada", 300, None)), true), raw(31, Kind::Follow, "fay", None, false)]);
    newer.iter_mut().for_each(|p| p.created_at += 1);
    w.script.newer.lock().unwrap().push((NOTIFICATIONS.into(), super::Fresh { posts: newer, top: Some("33".into()), gap: None }));
    w.click("check-now");
    w.until_waiting(&notifications_ref(), 1);
    assert_eq!(w.app.social.timelines[&(notifications_ref().account, NOTIFICATIONS.to_string())].waiting.len(), 2);
    assert_eq!(w.title(0), "Mastodon · Notifications (1)", "the title counts the unread ones");
    assert!(w.rects_has("new-posts"));
    // nothing is read while it waits
    w.frames(5);
    assert_eq!((w.script.marks.lock().unwrap().len(), w.app.social.timelines[&(notifications_ref().account, NOTIFICATIONS.to_string())].marked), (1, 30));
    // shown: the server is told about the newest one
    w.click("new-posts");
    w.wait_for("the marker was not sent", |w| w.script.marks.lock().unwrap().len() == 2);
    assert_eq!(*w.script.marks.lock().unwrap(), ["30", "33"]);
    assert_eq!(w.title(0), "Mastodon · Notifications");
}

// spec: SOC-073
#[test]
fn soc_073_the_posts_of_notifications_are_searchable_as_posts_and_a_follow_is_not_a_post() {
    let w = on_notifications("notif-index");
    let account = notifications_ref().account;
    let entries = feed_entries();
    // the reply and the favourited post are in the index under their own keys, without the notice
    for p in entries.iter().take(2) {
        let plain = Post { notice: None, ..p.clone() };
        assert!(w.app.social.index.contains(&account, &post_key(&plain)), "{}", p.id);
        assert_eq!(w.app.social.index.get(&account, &post_key(&plain)).unwrap().notice, None);
    }
    assert_eq!(w.app.social.index.count(None), 2, "the follow is not a post");
    assert!(w.app.social.index.search_wild("DEE", 5).is_empty());
    assert_eq!(w.app.social.index.search_wild("text of own1", 5)[0].feed, NOTIFICATIONS);
}

// spec: SOC-074
#[test]
fn soc_074_your_posts_is_a_feed_like_the_others_and_a_post_sent_from_shale_waits_in_it() {
    let mut w = W::new("own-feed");
    let account = home().account;
    let mut mine = post("m1", "ada", crate::calendar::time::now() - 500, None);
    mine.author.handle = "ada@demo.social".into();
    *w.script.first_page.lock().unwrap() = Some(Page { posts: vec![mine], ..Default::default() });
    w.click("feed-combo");
    w.click(&format!("feed:{OWN}"));
    let key = (account.clone(), OWN.to_string());
    w.wait_for("your posts did not load", |w| w.app.social.timelines.get(&key).is_some_and(|t| t.loaded));
    w.frames(3);
    assert_eq!(w.title(0), "Mastodon · Your posts");
    assert!(w.rects_has("save:0") && !w.rects_has("notice:0"), "plain cards");
    assert!(w.app.social.index.contains(&account, &post_key(&post("m1", "ada", 0, None))), "indexed like the posts of other feeds");
    // a new post from Shale waits in Home and in Your posts, behind the button
    let mut sent = post("sent1", "ada", crate::calendar::time::now(), None);
    sent.author.handle = "ada@demo.social".into();
    w.app.social.sent(&account, "home", Ok(Sent::Posted(Box::new(sent))));
    assert_eq!(w.app.social.waiting_count(&account, OWN), 1);
    assert_eq!(w.app.social.waiting_count(&account, "home"), 1);
    assert_eq!(w.app.social.waiting_count(&account, "list:1"), 0);
    w.frames(2);
    assert!(w.rects_has("new-posts"));
    assert_eq!(w.title(0), "Mastodon · Your posts (1)");
}

// spec: SOC-075, SOC-005
#[test]
fn soc_075_a_saved_tab_names_the_new_feeds_and_a_removed_account_takes_them_with_it() {
    for feed in [NOTIFICATIONS, OWN] {
        let r = FeedRef::new("mastodon:ada@demo.social", feed);
        assert_eq!(serde_json::from_str::<FeedRef>(&serde_json::to_string(&r).unwrap()).unwrap(), r);
    }
    let mut w = on_notifications("notif-saved");
    // the tab is restored: the feed list has the feed again, so the tab shows it and not Home
    let mut again = W::new("notif-saved-2");
    again.app.show_view(View::Social(Some(notifications_ref())), false);
    again.frames(4);
    assert_eq!(again.title(again.app.tabbar.active), "Mastodon · Notifications");
    assert!(matches!(again.app.social.feeds.get(&notifications_ref().account), Some(super::state::FeedsState::Ready(l)) if l.iter().any(|f: &Feed| f.id == NOTIFICATIONS)));
    // a feed the server's list does not have shows the account's first feed
    let account = notifications_ref().account;
    again.app.social.feed_of.clear();
    assert_eq!(again.app.social.resolve_feed(&account, "no-such-feed", None).unwrap().id, "home");
    // removing the account forgets the feed and its posts
    assert!(!w.app.social_dialog_action(crate::social::accounts_ui::DialogAction::Remove(account.clone())));
    assert!(!w.app.social.timelines.keys().any(|k| k.0 == account) && w.app.social.index.count(None) == 0);
    w.frames(3);
    assert!(w.rects_has("add-account"), "no account: the feed view asks for one");
}

// spec: SOC-072
#[test]
fn soc_072_unread_waiting_entries_are_counted_and_the_marker_is_sent_again_after_a_reload() {
    let mut w = on_notifications("notif-count");
    let key = (notifications_ref().account, NOTIFICATIONS.to_string());
    w.wait_for("the marker was not sent", |w| !w.script.marks.lock().unwrap().is_empty());
    w.app.social.timelines.get_mut(&key).unwrap().waiting = entries(Network::Mastodon, vec![raw(40, Kind::Follow, "x", None, false), raw(41, Kind::Follow, "y", None, true), raw(42, Kind::Follow, "z", None, true)]);
    assert_eq!(w.app.social.waiting_count(&key.0, NOTIFICATIONS), 2);
    // posts that are no notifications count as before
    w.app.social.timelines.insert(("other".into(), "home".into()), Timeline { waiting: vec![post("a", "x", 1, None), post("b", "x", 2, None)], ..Default::default() });
    assert_eq!(w.app.social.waiting_count("other", "home"), 2);
    // a reload starts again: the list is new, so the newest one is told to the server once more
    w.app.social.timelines.get_mut(&key).unwrap().waiting.clear();
    *w.script.first_page.lock().unwrap() = Some(Page { posts: feed_entries(), ..Default::default() });
    w.click("feed-menu");
    w.click("reload");
    w.wait_for("the second marker was not sent", |w| w.script.marks.lock().unwrap().len() == 2);
}

/// Entries for the posts of `own1`: one like per person, as raws are numbered.
fn likes(ids: &[(i64, &str)]) -> Vec<Post> {
    let own = post("own1", "ada", crate::calendar::time::now() - 100, None);
    entries(Network::Mastodon, ids.iter().map(|(id, who)| raw(*id, Kind::Like, who, Some(own.clone()), true)).collect())
}

// spec: SOC-071
#[test]
fn soc_071_a_group_that_a_page_boundary_cut_is_one_entry_again() {
    let key = (notifications_ref().account, NOTIFICATIONS.to_string());
    let who = |w: &W| w.app.social.timelines[&key].posts.iter().map(|p| p.notice.as_ref().unwrap().who_text()).collect::<Vec<_>>();
    // the boundary is between a follow and likes: nothing to merge
    let mut first = likes(&[(20, "cy")]);
    first.extend(entries(Network::Mastodon, vec![raw(10, Kind::Follow, "dee", None, false)]));
    let mut next = likes(&[(19, "bob"), (18, "eve")]);
    next.extend(entries(Network::Mastodon, vec![raw(9, Kind::Follow, "fay", None, false)]));
    let mut w = on_page("notif-page-cut", Page { posts: first, cursor: Some("c1".into()), top: Some("20".into()) }, Some(Page { posts: next, ..Default::default() }));
    w.wait_for("the next page did not come", |w| w.app.social.timelines[&key].cursor.is_none());
    assert_eq!(who(&w), ["CY", "DEE", "BOB and EVE", "FAY"], "likes that are not neighbours stay apart");
    // the cut falls between two likes of the same post: one entry, with its key, place and newest time
    let mut w = on_page("notif-page-cut-2", Page { posts: likes(&[(20, "cy")]), cursor: Some("c1".into()), ..Default::default() }, Some(Page { posts: likes(&[(19, "bob")]), ..Default::default() }));
    w.wait_for("the next page did not come", |w| w.app.social.timelines[&key].cursor.is_none());
    let posts = &w.app.social.timelines[&key].posts;
    assert_eq!(posts.len(), 1, "one entry");
    let n = posts[0].notice.as_ref().unwrap();
    assert_eq!((n.who_text().as_str(), n.ids.len(), n.rank, n.id.as_str()), ("CY and BOB", 2, 20, "20"));
}

// spec: SOC-071
#[test]
fn soc_071_a_group_that_a_check_cut_is_one_entry_again_while_it_waits_and_when_it_is_shown() {
    let mut first = likes(&[(20, "cy")]);
    first.extend(entries(Network::Mastodon, vec![raw(10, Kind::Follow, "dee", None, false)]));
    let mut w = on_page("notif-check-cut", Page { posts: first, top: Some("20".into()), ..Default::default() }, None);
    let key = (notifications_ref().account, NOTIFICATIONS.to_string());
    let fresh = |posts: Vec<Post>, top: &str| super::Fresh { posts, top: Some(top.into()), gap: None };
    w.script.newer.lock().unwrap().push((NOTIFICATIONS.into(), fresh(likes(&[(26, "eve")]), "26")));
    w.click("check-now");
    w.until_waiting(&notifications_ref(), 1);
    // a second check: a like of the same post joins the one that waits
    w.script.newer.lock().unwrap().push((NOTIFICATIONS.into(), fresh(likes(&[(27, "fay")]), "27")));
    w.click("check-now");
    w.wait_for("the second check did not come", |w| w.app.social.timelines[&key].waiting.first().is_some_and(|p| p.notice.as_ref().unwrap().ids.len() == 2));
    assert_eq!(w.app.social.timelines[&key].waiting.len(), 1);
    assert_eq!(w.title(0), "Mastodon · Notifications (1)");
    // shown: it joins the entry of the list that is about the same post
    w.click("new-posts");
    let t = &w.app.social.timelines[&key];
    assert_eq!(t.posts.len(), 2, "the follow and one entry for the three likes");
    let n = t.posts[0].notice.as_ref().unwrap();
    assert_eq!((n.who_text().as_str(), n.ids.len(), n.rank), ("FAY, EVE and 1 other", 3, 27));
    assert_eq!(super::notifications::newest(&t.posts).unwrap().mark, "27", "the server is told the newest");
}
