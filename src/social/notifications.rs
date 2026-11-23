//! Notifications as a feed (SOC-068 to SOC-073) and the two extra feeds of every account, "Notifications"
//! and "Your posts" (SOC-074).
//!
//! A notification is an entry of the list like a post is: a `Post` whose `notice` says why it is
//! there and who did it. The entry is the post the notification is about (the liked post, the
//! reply, the quote), or, for a follow, a made-up post that only holds the person. So paging,
//! checks, waiting entries, gaps and the post index work as they do for a timeline; only the keys
//! (`state::post_key` adds the notice) and the drawing (`notifications_ui`) know the difference.
//!
//! Mastodon: `GET /api/v1/notifications` (`max_id` pages, `min_id` checks; the grouped API of
//! `/api/v2/notifications` gives no more than the grouping below, so the one API every server has
//! is used), read marker `GET` / `POST /api/v1/markers`. Bluesky:
//! `app.bsky.notification.listNotifications` (cursor), the posts through `app.bsky.feed.getPosts`,
//! read through `app.bsky.notification.updateSeen`.

use super::bluesky::{parse_author, states, Api};
use super::mastodon::{newer_pages, newest_id, parse_account, parse_status};
use super::*;
use serde_json::json;
use std::collections::{HashMap, HashSet};

/// The id of the Notifications feed of both networks.
pub const NOTIFICATIONS: &str = "notifications";
/// The id of the feed of the account's own posts of both networks.
pub const OWN: &str = "own";

/// The feeds every account has after Home / Following (SOC-068).
pub fn extra_feeds() -> [Feed; 2] {
    [Feed { id: NOTIFICATIONS.into(), title: "Notifications".into() }, Feed { id: OWN.into(), title: "Your posts".into() }]
}

/// What happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Mention,
    Reply,
    Quote,
    Like,
    Boost,
    Follow,
    FollowRequest,
    /// a poll the account took part in ended
    Poll,
    /// a post the account boosted was edited
    Update,
    /// somebody the account asked to be told about posted
    Status,
    /// somebody joined with the account's starter pack (Bluesky)
    Joined,
}

impl Kind {
    /// The notification is about a person, not about a post: there is no post to show.
    pub fn is_person(self) -> bool {
        matches!(self, Kind::Follow | Kind::FollowRequest | Kind::Joined)
    }

    /// Entries of this kind that come one after the other for the same post are one entry.
    fn groups(self) -> bool {
        matches!(self, Kind::Like | Kind::Boost)
    }

    /// A mark for the row.
    pub fn icon(self) -> &'static str {
        match self {
            Kind::Mention => "@",
            Kind::Reply => "↩",
            Kind::Quote => "“",
            Kind::Like => "♥",
            Kind::Boost => "🔁",
            Kind::Follow | Kind::FollowRequest | Kind::Joined => "+",
            Kind::Poll => "☰",
            Kind::Update => "✎",
            Kind::Status => "•",
        }
    }

    /// What the people did, after their names.
    fn verb(self, network: Network) -> &'static str {
        let m = network == Network::Mastodon;
        match self {
            Kind::Mention => "mentioned you",
            Kind::Reply => "replied to you",
            Kind::Quote => "quoted your post",
            Kind::Like if m => "favourited your post",
            Kind::Like => "liked your post",
            Kind::Boost if m => "boosted your post",
            Kind::Boost => "reposted your post",
            Kind::Follow => "followed you",
            Kind::FollowRequest => "asked to follow you",
            Kind::Poll => "'s poll has ended",
            Kind::Update => "edited a post",
            Kind::Status => "posted",
            Kind::Joined => "joined with your starter pack",
        }
    }
}

/// Why an entry of the Notifications feed is there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Notice {
    /// the newest notification of the entry: Mastodon's id, the Bluesky record address
    pub id: String,
    pub kind: Kind,
    /// everybody who did it, newest first (more than one for likes and boosts of the same post)
    pub who: Vec<Author>,
    /// when the newest of them did it, seconds since 1970
    pub at: i64,
    /// the server had not been told that it was read when the list was loaded (SOC-072)
    pub unread: bool,
    /// what the server is told to mark it read: Mastodon's id, Bluesky's `indexedAt`
    pub mark: String,
    /// `mark` as a number, to find the newest
    pub rank: i64,
    /// the ids of every notification in the entry (a check meets none of them twice)
    pub ids: Vec<String>,
}

impl Notice {
    /// "Ada", "Ada and Bo", "Ada, Bo and 3 others".
    pub fn who_text(&self) -> String {
        let name = |i: usize| self.who.get(i).map(|a| a.name().to_string()).unwrap_or_default();
        match self.who.len() {
            0 => String::new(),
            1 => name(0),
            2 => format!("{} and {}", name(0), name(1)),
            n => format!("{}, {} and {} other{}", name(0), name(1), n - 2, if n == 3 { "" } else { "s" }),
        }
    }

    /// The sentence of the row, "Ada and Bo favourited your post".
    pub fn line(&self, network: Network) -> String {
        match self.kind {
            Kind::Poll => format!("{}{}", self.who_text(), self.kind.verb(network)),
            k => format!("{} {}", self.who_text(), k.verb(network)),
        }
    }
}

/// A notification as the servers tell it, before the entries are made.
pub(super) struct Raw {
    pub id: String,
    pub mark: String,
    pub rank: i64,
    pub kind: Kind,
    pub who: Author,
    pub at: i64,
    pub unread: bool,
    /// the post it is about (None for a person)
    pub post: Option<Post>,
}

/// The post of an entry about a person: only the author is shown.
fn person_post(network: Network, r: &Raw) -> Post {
    Post {
        network,
        id: format!("notif:{}", r.id),
        cid: String::new(),
        url: r.who.profile_url.clone(),
        author: r.who.clone(),
        created_at: r.at,
        text_markdown: String::new(),
        content_warning: None,
        media: vec![],
        quote: None,
        link_card: None,
        reply_to: None,
        reposted_by: None,
        reply_count: 0,
        boost_count: 0,
        like_count: 0,
        mine: Mine::default(),
        reply_root: None,
        visibility: None,
        poll: None,
        notice: None,
    }
}

/// The entries of a page of notifications, newest first: likes and boosts that come one after the
/// other for the same post are one entry (SOC-071).
pub(super) fn entries(network: Network, raws: Vec<Raw>) -> Vec<Post> {
    let mut out: Vec<Post> = vec![];
    for r in raws {
        let merged = match out.last_mut() {
            Some(last) => match last.notice.as_deref_mut() {
                Some(n) if r.kind.groups() && n.kind == r.kind && r.post.as_ref().is_some_and(|p| p.id == last.id) => {
                    n.ids.push(r.id.clone());
                    n.unread |= r.unread;
                    if !n.who.iter().any(|a| a.id == r.who.id) {
                        n.who.push(r.who.clone());
                    }
                    true
                }
                _ => false,
            },
            None => false,
        };
        if merged {
            continue;
        }
        let notice = Notice { id: r.id.clone(), kind: r.kind, who: vec![r.who.clone()], at: r.at, unread: r.unread, mark: r.mark.clone(), rank: r.rank, ids: vec![r.id.clone()] };
        let mut post = r.post.clone().unwrap_or_else(|| person_post(network, &r));
        post.notice = Some(Box::new(notice));
        out.push(post);
    }
    out
}

/// Two neighbouring entries are about the same thing and one entry: likes or boosts of the same
/// post (a page boundary or a later check cut the group, SOC-071).
pub fn same_group(a: &Post, b: &Post) -> bool {
    match (a.notice.as_deref(), b.notice.as_deref()) {
        (Some(x), Some(y)) => x.kind.groups() && x.kind == y.kind && a.id == b.id,
        _ => false,
    }
}

/// Take the entry `other` into `keep` (`same_group`): the people, the notifications, the unread
/// state; the time and the mark are the newer one's. `keep` keeps its key and its place in the
/// list, so nothing that is shown moves, and the entries between the two stay in date order
/// because the two were neighbours.
pub fn merge_into(keep: &mut Post, other: Post) {
    let (Some(k), Some(o)) = (keep.notice.as_deref_mut(), other.notice.map(|n| *n)) else { return };
    let other_newer = o.rank > k.rank;
    let (mut first, second) = if other_newer { (o.who.clone(), k.who.clone()) } else { (k.who.clone(), o.who.clone()) };
    for a in second {
        if !first.iter().any(|x| x.id == a.id) {
            first.push(a);
        }
    }
    k.who = first;
    k.unread |= o.unread;
    let new_ids: Vec<String> = o.ids.iter().filter(|i| !k.ids.contains(i)).cloned().collect();
    k.ids.extend(new_ids);
    if other_newer {
        (k.at, k.mark, k.rank) = (o.at, o.mark, o.rank);
    }
}

/// The newest notification of the entries (what the server is told is read).
pub fn newest(posts: &[Post]) -> Option<&Notice> {
    posts.iter().filter_map(|p| p.notice.as_deref()).max_by_key(|n| n.rank)
}

/// The most pages one check asks for.
const MAX_CHECK_PAGES: usize = 12;

// ---------------------------------------------------------------- Mastodon

/// The kind of a Mastodon notification; None for the ones that are about the server (reports,
/// sign-ups, moderation) and for types this version does not know.
fn mastodon_kind(item: &Value) -> Option<Kind> {
    Some(match item["type"].as_str()? {
        "mention" if !text(&item["status"]["in_reply_to_id"]).is_empty() => Kind::Reply,
        "mention" => Kind::Mention,
        "status" => Kind::Status,
        "reblog" => Kind::Boost,
        "favourite" => Kind::Like,
        "follow" => Kind::Follow,
        "follow_request" => Kind::FollowRequest,
        "poll" => Kind::Poll,
        "update" | "quoted_update" => Kind::Update,
        "quote" => Kind::Quote,
        _ => return None,
    })
}

/// The entries of the notifications of one answer. `marker`: the id the account has read up to.
pub(super) fn mastodon_entries(items: &[Value], server: &str, marker: i64) -> Vec<Post> {
    let raws = items
        .iter()
        .filter_map(|item| {
            let kind = mastodon_kind(item)?;
            let id = text(&item["id"]);
            let post = if kind.is_person() { None } else { Some(parse_status(&item["status"], server)?) };
            let rank = id.parse::<i64>().unwrap_or(0);
            Some(Raw { mark: id.clone(), id, rank, kind, who: parse_account(&item["account"], server), at: parse_rfc3339(&text(&item["created_at"])).unwrap_or(0), unread: rank > marker, post })
        })
        .collect();
    entries(Network::Mastodon, raws)
}

/// The id the account has read its notifications up to. 0 when it never did; when the server does
/// not say, everything counts as read (no dots rather than wrong ones).
fn mastodon_marker(server: &str, token: &str) -> i64 {
    match api("GET", &format!("{server}/api/v1/markers?timeline%5B%5D=notifications"), Some(token), None) {
        Ok(v) => v["notifications"]["last_read_id"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
        Err(_) => i64::MAX,
    }
}

/// One page (20) of notifications. `cursor`: the last notification id of the page before.
pub fn mastodon_page(server: &str, token: &str, cursor: Option<&str>) -> Result<Page, String> {
    let mut params = vec![("limit", "20")];
    if let Some(c) = cursor.filter(|c| !c.is_empty()) {
        params.push(("max_id", c));
    }
    let v = api("GET", &format!("{server}/api/v1/notifications{}", query(&params)), Some(token), None)?;
    let items = v.as_array().ok_or("the server's answer is not a list of notifications")?;
    let marker = mastodon_marker(server, token);
    let next = items.last().map(|s| text(&s["id"])).filter(|c| !c.is_empty());
    Ok(Page { posts: mastodon_entries(items, server, marker), cursor: next, top: newest_id(items) })
}

/// The notifications newer than `top` (see `mastodon::newer`).
pub fn mastodon_newer(server: &str, token: &str, top: Option<&str>, cap: usize) -> Result<Fresh, String> {
    let marker = mastodon_marker(server, token);
    newer_pages(
        top,
        cap,
        |min| {
            let mut params = vec![("limit", "40")];
            if let Some(m) = min {
                params.push(("min_id", m));
            }
            Ok(api("GET", &format!("{server}/api/v1/notifications{}", query(&params)), Some(token), None)?)
        },
        |items| mastodon_entries(items, server, marker),
    )
}

/// Tell the server that the notifications up to the id `mark` were read.
pub fn mastodon_mark(server: &str, token: &str, mark: &str) -> Result<(), String> {
    if mark.is_empty() || !mark.chars().all(|c| c.is_ascii_digit()) {
        return Err("bad notification id".into());
    }
    let server = clean_server(server)?;
    api("POST", &format!("{server}/api/v1/markers"), Some(token), Some(&json!({"notifications": {"last_read_id": mark}})))?;
    Ok(())
}

// ---------------------------------------------------------------- Bluesky

/// The kind of a Bluesky notification and the address of the post it is about; None for the ones
/// this version does not show (verifications and the like).
fn bluesky_kind(item: &Value) -> Option<(Kind, Option<String>)> {
    let (uri, subject) = (text(&item["uri"]), text(&item["reasonSubject"]));
    Some(match item["reason"].as_str()? {
        "like" | "like-via-repost" => (Kind::Like, Some(subject)),
        "repost" | "repost-via-repost" => (Kind::Boost, Some(subject)),
        "mention" => (Kind::Mention, Some(uri)),
        "reply" => (Kind::Reply, Some(uri)),
        "quote" => (Kind::Quote, Some(uri)),
        "subscribed-post" => (Kind::Status, Some(uri)),
        "follow" => (Kind::Follow, None),
        "starterpack-joined" => (Kind::Joined, None),
        _ => return None,
    })
}

/// The entries of a page of notifications: the posts they are about are fetched in batches. An
/// entry whose post is gone is left out.
fn bluesky_entries(api: &mut Api, items: &[Value]) -> Result<Vec<Post>, String> {
    let kinds: Vec<Option<(Kind, Option<String>)>> = items.iter().map(bluesky_kind).collect();
    let mut uris: Vec<String> = kinds.iter().flatten().filter_map(|(_, u)| u.clone()).filter(|u| u.starts_with("at://")).collect();
    uris.sort();
    uris.dedup();
    let posts: HashMap<String, Post> = states(api, &uris)?.into_iter().map(|p| (p.id.clone(), p)).collect();
    let raws = items
        .iter()
        .zip(kinds)
        .filter_map(|(item, k)| {
            let (kind, about) = k?;
            let post = match about {
                Some(u) => Some(posts.get(&u)?.clone()),
                None => None,
            };
            let mark = text(&item["indexedAt"]);
            let at = parse_rfc3339(&mark).unwrap_or(0);
            Some(Raw { id: text(&item["uri"]), mark, rank: at, kind, who: parse_author(&item["author"]), at, unread: !item["isRead"].as_bool().unwrap_or(false), post })
        })
        .collect();
    Ok(entries(Network::Bluesky, raws))
}

/// One page (30) of notifications and the cursor of the next.
fn bluesky_list(api: &mut Api, cursor: Option<&str>) -> Result<(Vec<Value>, Option<String>), String> {
    let mut params: Vec<(&str, &str)> = vec![("limit", "30")];
    if let Some(c) = cursor.filter(|c| !c.is_empty()) {
        params.push(("cursor", c));
    }
    let v = api.get("app.bsky.notification.listNotifications", &params)?;
    Ok((v["notifications"].as_array().cloned().unwrap_or_default(), Some(text(&v["cursor"])).filter(|c| !c.is_empty())))
}

/// One page of notifications.
pub fn bluesky_page(api: &mut Api, cursor: Option<&str>) -> Result<Page, String> {
    let (items, next) = bluesky_list(api, cursor)?;
    Ok(Page { posts: bluesky_entries(api, &items)?, cursor: next, top: None })
}

/// The notifications newer than the known ones: pages from the top up to the first notification
/// that an entry of the list already covers (`known` holds the ids of those, `Notice::ids`), so a
/// new like of a post whose likes are one entry does not make that entry twice. At most `cap`
/// entries; if that limit comes first, `Fresh::gap` has the cursor to go on with.
pub fn bluesky_newer(api: &mut Api, known: &HashSet<String>, cap: usize) -> Result<Fresh, String> {
    let mut fresh: Vec<Value> = vec![];
    let mut cursor: Option<String> = None;
    let mut gap = None;
    for _ in 0..MAX_CHECK_PAGES {
        let (items, next) = bluesky_list(api, cursor.as_deref())?;
        let met = items.iter().position(|i| known.contains(&text(&i["uri"])));
        fresh.extend(items.into_iter().take(met.unwrap_or(usize::MAX)));
        if met.is_some() || next.is_none() {
            gap = None;
            break;
        }
        cursor = next;
        gap = cursor.clone();
        if fresh.len() >= cap {
            break;
        }
    }
    let mut posts = vec![];
    for chunk in fresh.chunks(100) {
        posts.extend(bluesky_entries(api, chunk)?);
    }
    Ok(Fresh { posts, top: None, gap })
}

/// Tell the server that the notifications up to `seen_at` (the `indexedAt` of the newest) were read.
pub fn bluesky_mark(api: &mut Api, seen_at: &str) -> Result<(), String> {
    if seen_at.is_empty() {
        return Err("bad time".into());
    }
    api.call("POST", "app.bsky.notification.updateSeen", &[], Some(&json!({"seenAt": seen_at})))?;
    Ok(())
}
