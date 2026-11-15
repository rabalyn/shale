//! Mastodon: sign-in (OAuth with a code the user copies from the browser, or a pasted access token),
//! timelines (Home and the user's lists), boosts, and the thread of a post.
//!
//! Sign-in steps: `register_app` once per server → open `authorize_url` in the browser → the
//! user copies the code the server shows → `exchange_code` gives the access token → `verify`
//! checks it and tells the user's name. The token only needs the `read` permission.

use super::markup::html_to_markdown;
use super::*;
use serde_json::json;

const REDIRECT: &str = "urn:ietf:wg:oauth:2.0:oob";
const SCOPE: &str = "read";
const PAGE_SIZE: &str = "20";

/// What `register_app` gets from the server (the secret is not shown in debug output).
#[derive(Clone, PartialEq)]
pub struct AppCredentials {
    pub client_id: String,
    pub client_secret: Secret,
}

impl std::fmt::Debug for AppCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppCredentials").field("client_id", &self.client_id).field("client_secret", &cal_dav::redacted(self.client_secret.expose())).finish()
    }
}

/// Who a token belongs to (the answer of `verify`).
#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    /// "ada@mastodon.social"
    pub handle: String,
    pub display_name: String,
}

fn base(server: &str) -> Result<String, String> {
    clean_server(server)
}

/// Step 1: tell the server that "Shale" wants to read the account (creates an app on that server).
pub fn register_app(server: &str) -> Result<AppCredentials, String> {
    let server = base(server)?;
    let body = json!({"client_name": "Shale", "redirect_uris": REDIRECT, "scopes": SCOPE});
    let mut v = api("POST", &format!("{server}/api/v1/apps"), None, Some(&body))?;
    let (id, secret) = (text(&v["client_id"]), secret_of(&v["client_secret"]));
    wipe_value(&mut v);
    if id.is_empty() || secret.is_empty() {
        return Err("the server did not accept the app registration (is this a Mastodon server?)".into());
    }
    Ok(AppCredentials { client_id: id, client_secret: secret })
}

/// Step 2: the address to open in the browser; the user logs in, allows access and gets a code.
pub fn authorize_url(server: &str, app: &AppCredentials) -> Result<String, String> {
    let server = base(server)?;
    Ok(format!("{server}/oauth/authorize{}", query(&[("client_id", &app.client_id), ("scope", SCOPE), ("redirect_uri", REDIRECT), ("response_type", "code")])))
}

/// Step 3: the code from the browser → the access token.
pub fn exchange_code(server: &str, app: &AppCredentials, code: &str) -> Result<Secret, String> {
    let server = base(server)?;
    let mut body = json!({"client_id": app.client_id, "client_secret": app.client_secret.expose(), "redirect_uri": REDIRECT, "grant_type": "authorization_code", "code": code.trim(), "scope": SCOPE});
    let answer = api("POST", &format!("{server}/oauth/token"), None, Some(&body));
    wipe_value(&mut body);
    let mut v = answer?;
    let tok = secret_of(&v["access_token"]);
    wipe_value(&mut v);
    if tok.is_empty() {
        Err("the server did not give an access token".into())
    } else {
        Ok(tok)
    }
}

/// Who the token belongs to; fails when the token or the server is wrong.
pub fn verify(server: &str, token: &str) -> Result<Identity, String> {
    let server = base(server)?;
    let v = api("GET", &format!("{server}/api/v1/accounts/verify_credentials"), Some(token), None)?;
    let acct = text(&v["acct"]);
    if acct.is_empty() {
        return Err("this does not look like a Mastodon account".into());
    }
    let handle = full_acct(&acct, &server);
    let name = text(&v["display_name"]);
    Ok(Identity { display_name: if name.is_empty() { acct } else { name }, handle })
}

/// "ada" on https://mastodon.social → "ada@mastodon.social"; "ada@other.org" stays.
fn full_acct(acct: &str, server: &str) -> String {
    if acct.contains('@') {
        acct.to_string()
    } else {
        format!("{acct}@{}", cal_dav::host_of(server))
    }
}

/// Home and the user's lists.
pub fn feeds(server: &str, token: &str) -> Result<Vec<Feed>, String> {
    let server = base(server)?;
    let mut out = vec![Feed { id: "home".into(), title: "Home".into() }];
    let lists = api("GET", &format!("{server}/api/v1/lists"), Some(token), None)?;
    for l in lists.as_array().into_iter().flatten() {
        let (id, title) = (text(&l["id"]), text(&l["title"]));
        if !id.is_empty() {
            out.push(Feed { id: format!("list:{id}"), title: if title.is_empty() { "List".into() } else { title } });
        }
    }
    Ok(out)
}

/// The API path of a feed.
fn timeline_path(feed: &Feed) -> Result<String, String> {
    match feed.id.strip_prefix("list:") {
        Some(id) if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()) => Ok(format!("timelines/list/{id}")),
        Some(_) => Err("bad list".into()),
        None => Ok("timelines/home".to_string()),
    }
}

/// The id of the newest of the timeline entries (ids are numbers, longer is newer).
fn newest_id(items: &[Value]) -> Option<String> {
    items.iter().map(|s| text(&s["id"])).filter(|c| !c.is_empty()).max_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)))
}

/// One page of a timeline (20 posts). `cursor`: the last id of the page before.
pub fn timeline(server: &str, token: &str, feed: &Feed, cursor: Option<&str>) -> Result<Page, String> {
    let server = base(server)?;
    let path = timeline_path(feed)?;
    let mut params = vec![("limit", PAGE_SIZE)];
    if let Some(c) = cursor.filter(|c| !c.is_empty()) {
        params.push(("max_id", c));
    }
    let v = api("GET", &format!("{server}/api/v1/{path}{}", query(&params)), Some(token), None)?;
    let items = v.as_array().ok_or("the server's answer is not a timeline")?;
    // the next page starts after the last status as the timeline lists it (a boost has its own id)
    let next = items.last().map(|s| text(&s["id"])).filter(|c| !c.is_empty());
    let posts: Vec<Post> = items.iter().filter_map(|s| parse_status(s, &server)).collect();
    Ok(Page { posts, cursor: next, top: newest_id(items) })
}

/// The most pages one check asks for.
const MAX_CHECK_PAGES: usize = 12;

/// The posts newer than the timeline entry `top`: `min_id` gives the page directly above it, and
/// the newest entry of that page is the `min_id` of the next, until a page is empty or `cap`
/// posts are there. What is found is always the block directly above `top`, so nothing is
/// missing in between; if the cap stops the check, the next check goes on from the new top.
pub fn newer(server: &str, token: &str, feed: &Feed, top: Option<&str>, cap: usize) -> Result<Fresh, String> {
    let server = base(server)?;
    let path = timeline_path(feed)?;
    newer_with(&server, top, cap, |min| {
        let mut params = vec![("limit", "40")];
        if let Some(m) = min {
            params.push(("min_id", m));
        }
        Ok(api("GET", &format!("{server}/api/v1/{path}{}", query(&params)), Some(token), None)?)
    })
}

/// `newer` with the requests left to the caller (`fetch(min_id)` returns the JSON answer).
pub(crate) fn newer_with(server: &str, top: Option<&str>, cap: usize, mut fetch: impl FnMut(Option<&str>) -> Result<Value, String>) -> Result<Fresh, String> {
    let mut posts: Vec<Post> = vec![];
    let mut min = top.map(String::from);
    let mut newest = None;
    for _ in 0..MAX_CHECK_PAGES {
        let v = fetch(min.as_deref())?;
        let items = v.as_array().ok_or("the server's answer is not a timeline")?;
        let Some(page_top) = newest_id(items) else { break };
        // this page is newer than what was found before it
        let mut page: Vec<Post> = items.iter().filter_map(|s| parse_status(s, server)).collect();
        page.append(&mut posts);
        posts = page;
        min = Some(page_top.clone());
        newest = Some(page_top);
        if posts.len() >= cap {
            break;
        }
    }
    Ok(Fresh { posts, top: newest, gap: None })
}

/// The self-thread and the replies of other people.
pub fn thread_full(server: &str, token: &str, post: &Post) -> Result<(Vec<Post>, Vec<Reply>), String> {
    let server = base(server)?;
    if post.id.is_empty() || !post.id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("bad post id".into());
    }
    let v = api("GET", &format!("{server}/api/v1/statuses/{}/context", post.id), Some(token), None)?;
    let t = thread_from_context(post, &v, &server);
    let r = replies_from_context(&t, &v, &server);
    Ok((t, r))
}

/// The replies of other people from a `/context` answer, as a tree (see `conversation`).
pub fn replies_from_context(thread: &[Post], context: &Value, server: &str) -> Vec<Reply> {
    let all: Vec<Post> = context["descendants"].as_array().into_iter().flatten().filter_map(|s| parse_status(s, server)).collect();
    conversation(thread, &all)
}

/// The thread from a `/context` answer. (Separate so it can be tested with fixtures.)
pub fn thread_from_context(post: &Post, context: &Value, server: &str) -> Vec<Post> {
    let list = |k: &str| -> Vec<Post> { context[k].as_array().into_iter().flatten().filter_map(|s| parse_status(s, server)).collect() };
    let mut root = post.clone();
    root.reposted_by = None;
    self_thread(root, list("ancestors"), list("descendants"))
}

/// A status as a `Post`. A boost shows the original with `reposted_by` set. None for broken data.
pub fn parse_status(s: &Value, server: &str) -> Option<Post> {
    if let Some(orig) = s.get("reblog").filter(|r| r.is_object()) {
        let mut p = parse_status(orig, server)?;
        p.reposted_by = Some(parse_account(&s["account"], server));
        return Some(p);
    }
    let id = text(&s["id"]);
    if id.is_empty() {
        return None;
    }
    let url = [text(&s["url"]), text(&s["uri"])].into_iter().find(|u| u.starts_with("http")).unwrap_or_default();
    let cw = text(&s["spoiler_text"]);
    let media: Vec<Media> = s["media_attachments"].as_array().into_iter().flatten().filter_map(|m| parse_media(m, &id)).collect();
    let card = &s["card"];
    let link_card = (card.is_object() && !text(&card["url"]).is_empty() && !text(&card["title"]).is_empty()).then(|| LinkCard { url: text(&card["url"]), title: text(&card["title"]), description: text(&card["description"]) });
    // Mastodon 4.4 quote posts
    let quoted = &s["quote"]["quoted_status"];
    let quote = if quoted.is_object() { parse_status(quoted, server).map(Box::new) } else { None };
    let mut body = html_to_markdown(&text(&s["content"]));
    if let Some(poll) = s["poll"].as_object() {
        for o in poll.get("options").and_then(|o| o.as_array()).into_iter().flatten() {
            body.push_str(&format!("\n- [ ] {}", text(&o["title"])));
        }
    }
    Some(Post {
        network: Network::Mastodon,
        id,
        cid: String::new(),
        url,
        author: parse_account(&s["account"], server),
        created_at: parse_rfc3339(&text(&s["created_at"])).unwrap_or(0),
        text_markdown: body.trim().to_string(),
        content_warning: Some(cw.trim().to_string()).filter(|c| !c.is_empty()),
        media,
        quote,
        link_card,
        reply_to: Some(text(&s["in_reply_to_id"])).filter(|c| !c.is_empty()),
        reposted_by: None,
        reply_count: count(&s["replies_count"]),
        boost_count: count(&s["reblogs_count"]),
        like_count: count(&s["favourites_count"]),
    })
}

fn parse_account(a: &Value, server: &str) -> Author {
    Author { id: text(&a["id"]), handle: full_acct(&text(&a["acct"]), server), display_name: text(&a["display_name"]), profile_url: text(&a["url"]), avatar_url: text(&a["avatar"]) }
}

fn parse_media(m: &Value, post_id: &str) -> Option<Media> {
    let kind = match m["type"].as_str()? {
        "image" => MediaKind::Image,
        "video" => MediaKind::Video,
        "gifv" => MediaKind::Gifv,
        "audio" => MediaKind::Audio,
        _ => MediaKind::Other,
    };
    let url = text(&m["url"]);
    if url.is_empty() {
        return None;
    }
    let ext = match kind {
        MediaKind::Image => "jpg",
        MediaKind::Video | MediaKind::Gifv => "mp4",
        MediaKind::Audio => "mp3",
        MediaKind::Other => "bin",
    };
    Some(Media { kind, preview_url: text(&m["preview_url"]), alt: text(&m["description"]), file_name_hint: file_name_from_url(&url, &format!("{post_id}.{ext}")), url, blob_cid: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cal_dav::tests::mock_http;

    pub(crate) fn status(id: &str, acct_id: &str, acct: &str, at: &str, reply_to: Option<&str>, html: &str) -> Value {
        json!({"id": id, "uri": format!("https://m.test/users/{acct}/statuses/{id}"), "url": format!("https://m.test/@{acct}/{id}"), "created_at": at, "in_reply_to_id": reply_to, "spoiler_text": "", "content": html,
               "account": {"id": acct_id, "acct": acct, "display_name": acct.to_uppercase(), "url": format!("https://m.test/@{acct}"), "avatar": "https://m.test/a.png"},
               "media_attachments": [], "replies_count": 1, "reblogs_count": 2, "favourites_count": 3})
    }

    #[test]
    fn status_parsing() {
        let mut s = status("10", "1", "ada", "2024-05-17T10:30:00.000Z", None, "<p>Hello <a href=\"https://m.test/tags/x\" class=\"mention hashtag\">#<span>x</span></a></p>");
        s["spoiler_text"] = json!("spoilers");
        s["media_attachments"] = json!([{"type": "image", "url": "https://files.test/m/a.png", "preview_url": "https://files.test/m/s_a.png", "description": "A cat"}, {"type": "gifv", "url": "https://files.test/m/g", "preview_url": ""}]);
        s["card"] = json!({"url": "https://example.com/x", "title": "Example", "description": "Desc"});
        let p = parse_status(&s, "https://m.test").unwrap();
        assert_eq!((p.id.as_str(), p.author.handle.as_str(), p.created_at, p.text_markdown.as_str()), ("10", "ada@m.test", 1_715_941_800, "Hello [#x](https://m.test/tags/x)"));
        assert_eq!(p.content_warning.as_deref(), Some("spoilers"));
        assert_eq!((p.reply_count, p.boost_count, p.like_count), (1, 2, 3));
        assert_eq!((p.media[0].kind, p.media[0].alt.as_str(), p.media[0].file_name_hint.as_str()), (MediaKind::Image, "A cat", "a.png"));
        assert_eq!((p.media[1].kind, p.media[1].file_name_hint.as_str()), (MediaKind::Gifv, "g.mp4"));
        assert_eq!(p.link_card.unwrap().title, "Example");
        // a remote author keeps their own server in the handle
        let r = parse_status(&status("11", "9", "bob@far.example", "2024-05-17T10:30:00Z", Some("10"), "<p>x</p>"), "https://m.test").unwrap();
        assert_eq!((r.author.handle.as_str(), r.reply_to.as_deref()), ("bob@far.example", Some("10")));
        // a boost shows the original, and who boosted it
        let boost = json!({"id": "99", "reblog": status("10", "1", "ada", "2024-05-17T10:30:00Z", None, "<p>orig</p>"), "account": {"id": "5", "acct": "cy", "display_name": "Cy", "url": "https://m.test/@cy", "avatar": ""}});
        let b = parse_status(&boost, "https://m.test").unwrap();
        assert_eq!((b.id.as_str(), b.text_markdown.as_str(), b.reposted_by.unwrap().handle.as_str()), ("10", "orig", "cy@m.test"));
        assert!(parse_status(&json!({"foo": 1}), "https://m.test").is_none());
    }

    #[test]
    fn thread_from_a_context_fixture() {
        let ctx = json!({
            "ancestors": [status("1", "2", "bob", "2024-05-17T10:00:00Z", None, "<p>question</p>"), status("2", "1", "ada", "2024-05-17T10:01:00Z", Some("1"), "<p>answer 1/3</p>")],
            "descendants": [
                status("5", "3", "cy", "2024-05-17T10:03:00Z", Some("3"), "<p>nice</p>"),
                status("4", "1", "ada", "2024-05-17T10:04:00Z", Some("3"), "<p>answer 3/3</p>"),
                status("6", "1", "ada", "2024-05-17T10:05:00Z", Some("5"), "<p>reply to cy</p>")]});
        let root = parse_status(&status("3", "1", "ada", "2024-05-17T10:02:00Z", Some("2"), "<p>answer 2/3</p>"), "https://m.test").unwrap();
        let t = thread_from_context(&root, &ctx, "https://m.test");
        assert_eq!(t.iter().map(|p| p.text_markdown.as_str()).collect::<Vec<_>>(), ["answer 1/3", "answer 2/3", "answer 3/3"]);
        // spec: SOC-019: cy's reply, and the author's own answer to it, but not the thread itself
        let r = replies_from_context(&t, &ctx, "https://m.test");
        assert_eq!(r.iter().map(|r| (r.post.text_markdown.as_str(), r.depth)).collect::<Vec<_>>(), [("nice", 0), ("reply to cy", 1)]);
    }

    #[test]
    fn sign_in_feeds_paging_thread_and_media_against_a_mock_server() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let page1: Vec<Value> = (0..6).rev().map(|i| status(&format!("{}", 100 + i), "1", "ada", "2024-05-17T10:00:00Z", None, &format!("<p>post {i}</p>"))).collect();
        let (p1, p2) = (Value::Array(page1.clone()), json!([]));
        let ctx = json!({"ancestors": [], "descendants": [status("101", "1", "ada", "2024-05-17T10:30:00Z", Some("100"), "<p>part two</p>")]});
        let (base, handle) = mock_http(8, move |method, path, headers, body| {
            let authed = headers.to_ascii_lowercase().contains("authorization: bearer tok-1");
            match (method, path) {
                ("POST", "/api/v1/apps") => (200, json!({"client_id": "cid", "client_secret": "csec"}).to_string()),
                ("POST", "/oauth/token") => {
                    assert!(body.contains("\"code\":\"abc\"") && body.contains("csec"), "{body}");
                    (200, json!({"access_token": "tok-1"}).to_string())
                }
                ("GET", "/api/v1/accounts/verify_credentials") if authed => (200, json!({"acct": "ada", "display_name": "Ada"}).to_string()),
                ("GET", "/api/v1/lists") if authed => (200, json!([{"id": "7", "title": "Friends"}]).to_string()),
                ("GET", "/api/v1/timelines/home?limit=20") if authed => (200, p1.to_string()),
                ("GET", "/api/v1/timelines/home?limit=20&max_id=100") if authed => (200, p2.to_string()),
                ("GET", "/api/v1/statuses/100/context") if authed => (200, ctx.to_string()),
                ("GET", "/files/a.png") => {
                    assert!(!headers.to_ascii_lowercase().contains("authorization"), "media gets no login: {headers}");
                    (200, "PNGDATA".into())
                }
                _ => (404, json!({"error": "nope"}).to_string()),
            }
        });
        let app = register_app(&base).unwrap();
        assert!(authorize_url(&base, &app).unwrap().contains("/oauth/authorize?client_id=cid&scope=read&redirect_uri=urn%3Aietf%3Awg%3Aoauth%3A2.0%3Aoob&response_type=code"));
        let tok = exchange_code(&base, &app, " abc ").unwrap();
        let who = verify(&base, tok.expose()).unwrap();
        assert_eq!((who.handle.as_str(), who.display_name.as_str()), ("ada@127.0.0.1", "Ada"));
        let mut c = Client::new(Account::new(Network::Mastodon, &base, &who.handle, &who.display_name), tok.expose());
        let feeds = c.feeds().unwrap();
        assert_eq!(feeds, vec![Feed { id: "home".into(), title: "Home".into() }, Feed { id: "list:7".into(), title: "Friends".into() }]);
        let page = c.timeline(&feeds[0], None).unwrap();
        assert_eq!(page.posts.len(), 6);
        assert_eq!(page.cursor.as_deref(), Some("100"));
        let next = c.timeline(&feeds[0], page.cursor.as_deref()).unwrap();
        assert!(next.posts.is_empty() && next.cursor.is_none());
        let first = page.posts.last().unwrap();
        assert_eq!(first.id, "100");
        let th = c.thread_with_replies(first).unwrap().0;
        assert_eq!(th.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["100", "101"]);
        let m = Media { kind: MediaKind::Image, url: format!("{base}/files/a.png"), preview_url: String::new(), alt: String::new(), file_name_hint: "a.png".into(), blob_cid: None };
        let (bytes, _) = c.download_media(first, &m).unwrap();
        assert_eq!(bytes, b"PNGDATA");
        let seen = handle.join().unwrap();
        assert_eq!(seen.len(), 8, "{seen:?}");
    }

    #[test]
    fn errors_are_readable() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let (base, h) = mock_http(1, |_, _, _, _| (401, json!({"error": "The access token is invalid"}).to_string()));
        let e = verify(&base, "bad").unwrap_err();
        assert!(e.contains("refused the login") && e.contains("invalid"), "{e}");
        h.join().unwrap();
        assert!(verify("http://mastodon.example", "x").unwrap_err().contains("https"));
        assert!(verify(&base, "to\nken").is_err());
    }

    fn entries(ids: &[u32]) -> Value {
        Value::Array(ids.iter().map(|i| status(&i.to_string(), "1", "ada", "2024-05-17T10:00:00Z", None, &format!("<p>post {i}</p>"))).collect())
    }

    // spec: SOC-026
    #[test]
    fn newer_posts_are_the_blocks_directly_above_the_newest_entry_until_a_page_is_empty() {
        // the server answers with the page above the id it is asked for (newest first inside a page)
        let mut asked: Vec<Option<String>> = vec![];
        let mut answers = vec![entries(&[105, 104, 103]), entries(&[108, 107, 106]), json!([])].into_iter();
        let f = newer_with("https://m.test", Some("102"), 200, |min| {
            asked.push(min.map(String::from));
            Ok(answers.next().unwrap())
        })
        .unwrap();
        assert_eq!(asked, [Some("102".to_string()), Some("105".into()), Some("108".into())], "each page continues from the newest entry of the one before");
        assert_eq!(f.posts.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["108", "107", "106", "105", "104", "103"], "newest first");
        assert_eq!((f.top.as_deref(), f.gap), (Some("108"), None));
        // nothing new: the top stays unknown to the caller (it keeps its own)
        let none = newer_with("https://m.test", Some("108"), 200, |_| Ok(json!([]))).unwrap();
        assert_eq!((none.posts.len(), none.top), (0, None));
        // the cap stops the paging; what was found is still the block right above the old top
        let mut n = 0;
        let capped = newer_with("https://m.test", Some("100"), 5, |_| {
            n += 1;
            Ok(entries(&[100 + 3 * n, 99 + 3 * n, 98 + 3 * n]))
        })
        .unwrap();
        assert_eq!((n, capped.posts.len(), capped.top.as_deref()), (2, 6, Some("106")));
        assert_eq!(capped.posts.last().unwrap().id, "101", "starts right above the old top");
        // an error is passed on and nothing is half-taken
        assert!(newer_with("https://m.test", Some("1"), 200, |_| Err("offline".into())).is_err());
    }

    // spec: SOC-026
    #[test]
    fn a_boost_has_its_own_entry_id_and_that_is_the_top() {
        let boost = json!({"id": "300", "reblog": status("10", "1", "ada", "2024-05-17T10:30:00Z", None, "<p>orig</p>"), "account": {"id": "5", "acct": "cy", "display_name": "Cy", "url": "https://m.test/@cy", "avatar": ""}});
        let f = newer_with("https://m.test", Some("200"), 200, |min| Ok(if min == Some("200") { json!([boost.clone(), status("250", "1", "ada", "2024-05-17T10:00:00Z", None, "<p>x</p>")]) } else { json!([]) })).unwrap();
        assert_eq!(f.top.as_deref(), Some("300"), "the boost's entry, not the id 10 of the post");
        assert_eq!(f.posts[0].id, "10");
        assert!(f.posts[0].reposted_by.is_some());
    }

    // spec: SOC-026
    #[test]
    fn the_request_for_newer_posts_carries_min_id_against_a_mock_server() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let page = entries(&[12, 11]);
        let (base, handle) = mock_http(2, move |_, path, headers, _| {
            assert!(headers.to_ascii_lowercase().contains("authorization: bearer tok-1"));
            match path {
                "/api/v1/timelines/home?limit=40&min_id=10" => (200, page.to_string()),
                "/api/v1/timelines/home?limit=40&min_id=12" => (200, "[]".into()),
                _ => (404, json!({"error": "nope"}).to_string()),
            }
        });
        let f = newer(&base, "tok-1", &Feed { id: "home".into(), title: "Home".into() }, Some("10"), 200).unwrap();
        assert_eq!((f.posts.len(), f.top.as_deref()), (2, Some("12")));
        assert_eq!(handle.join().unwrap().len(), 2);
    }
}
