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
    pub client_secret: String,
}

impl std::fmt::Debug for AppCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppCredentials").field("client_id", &self.client_id).field("client_secret", &cal_dav::redacted(&self.client_secret)).finish()
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
    let v = api("POST", &format!("{server}/api/v1/apps"), None, Some(&body))?;
    let (id, secret) = (text(&v["client_id"]), text(&v["client_secret"]));
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
pub fn exchange_code(server: &str, app: &AppCredentials, code: &str) -> Result<String, String> {
    let server = base(server)?;
    let body = json!({"client_id": app.client_id, "client_secret": app.client_secret, "redirect_uri": REDIRECT, "grant_type": "authorization_code", "code": code.trim(), "scope": SCOPE});
    let v = api("POST", &format!("{server}/oauth/token"), None, Some(&body))?;
    let tok = text(&v["access_token"]);
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
    if acct.contains('@') { acct.to_string() } else { format!("{acct}@{}", cal_dav::host_of(server)) }
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

/// One page of a timeline (20 posts). `cursor`: the last id of the page before.
pub fn timeline(server: &str, token: &str, feed: &Feed, cursor: Option<&str>) -> Result<Page, String> {
    let server = base(server)?;
    let path = match feed.id.strip_prefix("list:") {
        Some(id) if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()) => format!("timelines/list/{id}"),
        Some(_) => return Err("bad list".into()),
        None => "timelines/home".to_string(),
    };
    let mut params = vec![("limit", PAGE_SIZE)];
    if let Some(c) = cursor.filter(|c| !c.is_empty()) {
        params.push(("max_id", c));
    }
    let v = api("GET", &format!("{server}/api/v1/{path}{}", query(&params)), Some(token), None)?;
    let items = v.as_array().ok_or("the server's answer is not a timeline")?;
    // the next page starts after the last status as the timeline lists it (a boost has its own id)
    let next = items.last().map(|s| text(&s["id"])).filter(|c| !c.is_empty());
    let posts: Vec<Post> = items.iter().filter_map(|s| parse_status(s, &server)).collect();
    Ok(Page { posts, cursor: next })
}

/// The self-thread of a post (see `social::self_thread`).
pub fn thread(server: &str, token: &str, post: &Post) -> Result<Vec<Post>, String> {
    let server = base(server)?;
    if post.id.is_empty() || !post.id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("bad post id".into());
    }
    let v = api("GET", &format!("{server}/api/v1/statuses/{}/context", post.id), Some(token), None)?;
    Ok(thread_from_context(post, &v, &server))
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
        let who = verify(&base, &tok).unwrap();
        assert_eq!((who.handle.as_str(), who.display_name.as_str()), ("ada@127.0.0.1", "Ada"));
        let mut c = Client::new(Account::new(Network::Mastodon, &base, &who.handle, &who.display_name), &tok);
        let feeds = c.feeds().unwrap();
        assert_eq!(feeds, vec![Feed { id: "home".into(), title: "Home".into() }, Feed { id: "list:7".into(), title: "Friends".into() }]);
        let page = c.timeline(&feeds[0], None).unwrap();
        assert_eq!(page.posts.len(), 6);
        assert_eq!(page.cursor.as_deref(), Some("100"));
        let next = c.timeline(&feeds[0], page.cursor.as_deref()).unwrap();
        assert!(next.posts.is_empty() && next.cursor.is_none());
        let first = page.posts.last().unwrap();
        assert_eq!(first.id, "100");
        let th = c.thread(first).unwrap();
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
}
