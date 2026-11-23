//! Bluesky (AT Protocol, XRPC over HTTPS): sign-in with an app password, timelines and custom
//! feeds, posts with their facets (links, mentions, hashtags), pictures, videos, link cards and
//! quote posts, and the thread of a post.
//!
//! The login (`Session`) lives in memory only. `Api` keeps it for the caller and renews it
//! when it has expired (refresh token first, else a new login with the app password).

use super::actions::{iso_now, prepare, ActionError, Draft, Reacted, Reaction, TempFile, BLUESKY_IMAGE_BYTES, BLUESKY_MAX_CHARS};
use super::markup::{md_link, md_url};
use super::*;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

const PAGE_SIZE: &str = "30";

/// The login of an account (in memory only; the tokens are not shown in debug output).
#[derive(Clone, PartialEq)]
pub struct Session {
    /// where the account's data lives (from the DID document; the sign-in server when it names none)
    pub pds: String,
    pub did: String,
    pub handle: String,
    pub access_jwt: Secret,
    pub refresh_jwt: Secret,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("pds", &self.pds)
            .field("did", &self.did)
            .field("handle", &self.handle)
            .field("access_jwt", &dav::redacted(self.access_jwt.expose()))
            .field("refresh_jwt", &dav::redacted(self.refresh_jwt.expose()))
            .finish()
    }
}

/// What `Client` holds for a Bluesky account: the sign-in data and the session once there is one.
#[derive(Clone)]
pub struct Api {
    server: String,
    handle: String,
    password: Secret,
    pub session: Option<Session>,
}

impl std::fmt::Debug for Api {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Api").field("server", &self.server).field("handle", &self.handle).field("password", &dav::redacted(self.password.expose())).field("session", &self.session).finish()
    }
}

fn is_expired(e: &ApiError) -> bool {
    e.code == "ExpiredToken" || e.code == "InvalidToken"
}

fn session_from(v: &Value, server: &str) -> Result<Session, String> {
    let (access, refresh, did) = (secret_of(&v["accessJwt"]), secret_of(&v["refreshJwt"]), text(&v["did"]));
    if access.is_empty() || refresh.is_empty() || did.is_empty() {
        return Err("the server did not answer like a Bluesky server".into());
    }
    Ok(Session {
        pds: pds_of_did_doc(&v["didDoc"]).filter(|p| check_url(p).is_ok()).map(|p| p.trim_end_matches('/').to_string()).unwrap_or_else(|| server.to_string()),
        did,
        handle: text(&v["handle"]),
        access_jwt: access,
        refresh_jwt: refresh,
    })
}

/// `serviceEndpoint` of the `#atproto_pds` service of a DID document.
fn pds_of_did_doc(doc: &Value) -> Option<String> {
    doc["service"].as_array()?.iter().find(|s| text(&s["id"]).ends_with("#atproto_pds")).map(|s| text(&s["serviceEndpoint"])).filter(|e| !e.is_empty())
}

/// Sign in with the handle (or e-mail) and an app password (Settings → App passwords on bsky.app).
pub fn create_session(server: &str, handle: &str, app_password: &str) -> Result<Session, String> {
    let _step = tracing::info_span!("flow.social_bluesky.create_session", peer = "server").entered();
    let server = clean_server(server)?;
    let mut body = json!({"identifier": handle.trim().trim_start_matches('@'), "password": app_password});
    let answer = api("POST", &format!("{server}/xrpc/com.atproto.server.createSession"), None, Some(&body));
    wipe_value(&mut body);
    let mut v = answer.map_err(|e| if e.status == 401 || e.status == 400 { "Bluesky refused the handle or app password".to_string() } else { e.message })?;
    let session = session_from(&v, &server);
    wipe_value(&mut v);
    session
}

/// Get new tokens with the refresh token.
pub fn refresh_session(s: &Session) -> Result<Session, String> {
    let mut v = api("POST", &format!("{}/xrpc/com.atproto.server.refreshSession", s.pds), Some(s.refresh_jwt.expose()), None)?;
    let n = session_from(&v, &s.pds);
    wipe_value(&mut v);
    let mut n = n?;
    n.pds = s.pds.clone();
    Ok(n)
}

impl Api {
    pub fn new(server: &str, handle: &str, password: &str) -> Api {
        Api { server: server.trim().trim_end_matches('/').to_string(), handle: handle.trim().to_string(), password: Secret::new(password), session: None }
    }

    /// Sign in now (done by itself with the first call).
    pub fn login(&mut self) -> Result<&Session, String> {
        let s = create_session(&self.server, &self.handle, self.password.expose())?;
        Ok(self.session.insert(s))
    }

    /// Run a request with the session. When the login has expired: renew it (refresh token, else app password) and try once more.
    fn authed(&mut self, f: impl Fn(&Session) -> Result<Value, ApiError>) -> Result<Value, String> {
        if self.session.is_none() {
            self.login()?;
        }
        let once = |me: &Api| f(me.session.as_ref().ok_or_else(|| ApiError::plain("not signed in"))?);
        match once(self) {
            Err(e) if is_expired(&e) => {
                let renewed = self.session.as_ref().and_then(|s| refresh_session(s).ok());
                match renewed {
                    Some(s) => self.session = Some(s),
                    None => {
                        self.login()?;
                    }
                }
                Ok(once(self)?)
            }
            r => Ok(r?),
        }
    }

    /// One XRPC call (see `authed`).
    pub fn call(&mut self, method: &str, nsid: &str, params: &[(&str, &str)], body: Option<&Value>) -> Result<Value, String> {
        self.authed(|s| api(method, &format!("{}/xrpc/{nsid}{}", s.pds, query(params)), Some(s.access_jwt.expose()), body))
    }

    /// Upload a picture file as a blob (SOC-053): the answer holds the `blob` for the post's embed.
    pub fn upload_blob(&mut self, path: &std::path::Path, mime: &str) -> Result<Value, String> {
        self.authed(|s| api_upload("POST", &format!("{}/xrpc/com.atproto.repo.uploadBlob", s.pds), s.access_jwt.expose(), Some(mime), dav::Upload::Raw { path }))
    }

    /// The account's DID (signs in when that has not happened yet).
    pub fn did(&mut self) -> Result<String, String> {
        if self.session.is_none() {
            self.login()?;
        }
        self.session.as_ref().map(|s| s.did.clone()).ok_or_else(|| "not signed in".to_string())
    }

    pub fn get(&mut self, nsid: &str, params: &[(&str, &str)]) -> Result<Value, String> {
        self.call("GET", nsid, params, None)
    }
}

// ---------------------------------------------------------------- feeds and timelines

/// "Following" and the saved / pinned feeds of the account.
pub fn feeds(api: &mut Api) -> Result<Vec<Feed>, String> {
    let _step = tracing::info_span!("flow.social_bluesky.feeds", peer = "server").entered();
    let mut out = vec![Feed { id: "timeline".into(), title: "Following".into() }];
    let prefs = api.get("app.bsky.actor.getPreferences", &[])?;
    let uris = saved_feed_uris(&prefs);
    if uris.is_empty() {
        return Ok(out);
    }
    let params: Vec<(&str, &str)> = uris.iter().map(|u| ("feeds", u.as_str())).collect();
    let names: HashMap<String, String> = api.get("app.bsky.feed.getFeedGenerators", &params).map(|v| v["feeds"].as_array().into_iter().flatten().map(|f| (text(&f["uri"]), text(&f["displayName"]))).collect()).unwrap_or_default();
    for u in uris {
        let title = names.get(&u).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| u.rsplit('/').next().unwrap_or("Feed").to_string());
        out.push(Feed { id: u, title });
    }
    Ok(out)
}

/// The `at://` addresses of the custom feeds in the preferences: pinned ones first, at most 25.
pub fn saved_feed_uris(prefs: &Value) -> Vec<String> {
    let (mut pinned, mut saved) = (Vec::<String>::new(), Vec::<String>::new());
    let list = prefs["preferences"].as_array().cloned().unwrap_or_default();
    let v2 = list.iter().find(|p| text(&p["$type"]).ends_with("#savedFeedsPrefV2"));
    if let Some(v2) = v2 {
        for i in v2["items"].as_array().into_iter().flatten() {
            if text(&i["type"]) == "feed" {
                let u = text(&i["value"]);
                if i["pinned"].as_bool().unwrap_or(false) {
                    pinned.push(u)
                } else {
                    saved.push(u)
                }
            }
        }
    } else if let Some(old) = list.iter().find(|p| text(&p["$type"]).ends_with("#savedFeedsPref")) {
        pinned = old["pinned"].as_array().into_iter().flatten().map(text).collect();
        saved = old["saved"].as_array().into_iter().flatten().map(text).collect();
    }
    let mut out: Vec<String> = Vec::new();
    for u in pinned.into_iter().chain(saved) {
        if u.starts_with("at://") && u.contains("/app.bsky.feed.generator/") && !out.contains(&u) {
            out.push(u);
        }
    }
    out.truncate(25);
    out
}

/// One page (30 posts) of "Following" or a custom feed.
pub fn timeline(api: &mut Api, feed: &Feed, cursor: Option<&str>) -> Result<Page, String> {
    let _step = tracing::info_span!("flow.social_bluesky.timeline", peer = "server").entered();
    let mut params: Vec<(&str, &str)> = vec![("limit", PAGE_SIZE)];
    if let Some(c) = cursor.filter(|c| !c.is_empty()) {
        params.push(("cursor", c));
    }
    let v = if feed.id == "timeline" {
        api.get("app.bsky.feed.getTimeline", &params)?
    } else {
        if !feed.id.starts_with("at://") {
            return Err("bad feed".into());
        }
        params.push(("feed", &feed.id));
        api.get("app.bsky.feed.getFeed", &params)?
    };
    let posts = v["feed"].as_array().into_iter().flatten().filter_map(parse_feed_item).collect();
    Ok(Page { posts, cursor: Some(text(&v["cursor"])).filter(|c| !c.is_empty()), top: None })
}

/// The most pages one check asks for.
const MAX_CHECK_PAGES: usize = 12;

/// The posts newer than the known ones: the first pages of the feed, up to the first post that
/// is already known (`known`: `state::post_key`s). At most `cap` posts; if that limit comes
/// first, `Fresh::gap` has the cursor to go on with.
pub fn newer(api: &mut Api, feed: &Feed, known: &HashSet<String>, cap: usize) -> Result<Fresh, String> {
    newer_with(known, cap, |cursor| timeline(api, feed, cursor))
}

/// `newer` with the pages left to the caller (`page(cursor)`).
pub(crate) fn newer_with(known: &HashSet<String>, cap: usize, mut page: impl FnMut(Option<&str>) -> Result<Page, String>) -> Result<Fresh, String> {
    let mut posts: Vec<Post> = vec![];
    let mut have: HashSet<String> = HashSet::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_CHECK_PAGES {
        let pg = page(cursor.as_deref())?;
        for p in pg.posts {
            let key = super::state::post_key(&p);
            if known.contains(&key) {
                // met the posts of the list: everything above was new
                return Ok(Fresh { posts, ..Default::default() });
            }
            if have.insert(key) {
                posts.push(p);
            }
        }
        let Some(next) = pg.cursor else { return Ok(Fresh { posts, ..Default::default() }) };
        cursor = Some(next);
        if posts.len() >= cap {
            break;
        }
    }
    Ok(Fresh { posts, top: None, gap: cursor })
}

fn parse_feed_item(item: &Value) -> Option<Post> {
    let mut p = parse_post_view(&item["post"])?;
    let reason = &item["reason"];
    if text(&reason["$type"]).ends_with("#reasonRepost") {
        p.reposted_by = Some(parse_author(&reason["by"]));
    }
    Some(p)
}

// ---------------------------------------------------------------- threads

/// The self-thread and the replies of other people.
pub fn thread_full(api: &mut Api, post: &Post) -> Result<(Vec<Post>, Vec<Reply>), String> {
    let v = api.get("app.bsky.feed.getPostThread", &[("uri", &post.id), ("depth", "10"), ("parentHeight", "80")])?;
    let t = thread_from_response(post, &v);
    let r = replies_from_response(&t, &v);
    Ok((t, r))
}

/// The conversation around a post: the ancestors, and all replies as a tree (SOC-045).
pub fn thread_view(api: &mut Api, post: &Post) -> Result<ThreadView, String> {
    let v = api.get("app.bsky.feed.getPostThread", &[("uri", &post.id), ("depth", "10"), ("parentHeight", "80")])?;
    Ok(thread_view_from_response(post, &v))
}

/// The conversation from a `getPostThread` answer. (Separate so it can be tested with fixtures.)
pub fn thread_view_from_response(post: &Post, resp: &Value) -> ThreadView {
    let node = &resp["thread"];
    let root = parse_post_view(&node["post"]).unwrap_or_else(|| post.clone());
    let mut ancestors = Vec::new();
    let mut up = &node["parent"];
    while text(&up["$type"]).ends_with("#threadViewPost") {
        match parse_post_view(&up["post"]) {
            Some(p) => ancestors.push(p),
            None => break,
        }
        up = &up["parent"];
    }
    ancestors.reverse();
    let mut descendants = Vec::new();
    collect_replies(node, &mut descendants);
    thread_view_of(&root, ancestors, &descendants)
}

/// One post by its at:// address.
pub fn fetch_post(api: &mut Api, id: &str) -> Result<Post, String> {
    let v = api.get("app.bsky.feed.getPosts", &[("uris", id)])?;
    v["posts"].as_array().and_then(|a| a.first()).and_then(parse_post_view).ok_or_else(|| "the post is gone".to_string())
}

/// The replies of other people from a `getPostThread` answer, as a tree (see `conversation`).
pub fn replies_from_response(thread: &[Post], resp: &Value) -> Vec<Reply> {
    let mut all = Vec::new();
    collect_replies(&resp["thread"], &mut all);
    conversation(thread, &all)
}

/// The thread from a `getPostThread` answer. (Separate so it can be tested with fixtures.)
pub fn thread_from_response(post: &Post, resp: &Value) -> Vec<Post> {
    let node = &resp["thread"];
    let mut root = parse_post_view(&node["post"]).unwrap_or_else(|| post.clone());
    root.reposted_by = None;
    let mut ancestors = Vec::new();
    let mut up = &node["parent"];
    while text(&up["$type"]).ends_with("#threadViewPost") {
        match parse_post_view(&up["post"]) {
            Some(p) => ancestors.push(p),
            None => break,
        }
        up = &up["parent"];
    }
    ancestors.reverse();
    let mut descendants = Vec::new();
    collect_replies(node, &mut descendants);
    self_thread(root, ancestors, descendants)
}

fn collect_replies(node: &Value, out: &mut Vec<Post>) {
    for r in node["replies"].as_array().into_iter().flatten() {
        if text(&r["$type"]).ends_with("#threadViewPost") {
            out.extend(parse_post_view(&r["post"]));
            collect_replies(r, out);
        }
    }
}

// ---------------------------------------------------------------- posts

/// The address of a post on bsky.app.
pub fn web_url(handle_or_did: &str, at_uri: &str) -> String {
    format!("https://bsky.app/profile/{handle_or_did}/post/{}", at_uri.rsplit('/').next().unwrap_or(""))
}

fn parse_author(a: &Value) -> Author {
    let (did, handle) = (text(&a["did"]), text(&a["handle"]));
    let who = if handle.is_empty() || handle == "handle.invalid" { did.clone() } else { handle.clone() };
    Author { id: did, handle, display_name: text(&a["displayName"]), profile_url: format!("https://bsky.app/profile/{who}"), avatar_url: text(&a["avatar"]) }
}

/// A `postView` as a `Post`. None for blocked / missing posts and broken data.
pub fn parse_post_view(v: &Value) -> Option<Post> {
    let embeds: Vec<&Value> = if v["embed"].is_object() { vec![&v["embed"]] } else { vec![] };
    build_post(v, &v["record"], &embeds)
}

/// A post from its parts: `v` has uri, cid, author and the counts; `record` is the post record
/// (text, facets, createdAt); `embeds` are the embed views.
fn build_post(v: &Value, record: &Value, embeds: &[&Value]) -> Option<Post> {
    let (uri, cid) = (text(&v["uri"]), text(&v["cid"]));
    if !uri.starts_with("at://") || !record.is_object() {
        return None;
    }
    let author = parse_author(&v["author"]);
    let who = if author.handle.is_empty() || author.handle == "handle.invalid" { author.id.clone() } else { author.handle.clone() };
    let (mut media, mut quote, mut link_card) = (vec![], None, None);
    for e in embeds {
        embed_parts(e, &mut media, &mut quote, &mut link_card);
    }
    let created = parse_rfc3339(&text(&record["createdAt"])).or_else(|| parse_rfc3339(&text(&v["indexedAt"]))).unwrap_or(0);
    // moderation labels that hide the post behind a warning
    let cw = v["labels"].as_array().into_iter().flatten().map(|l| text(&l["val"])).find(|l| matches!(l.as_str(), "porn" | "sexual" | "nudity" | "graphic-media" | "gore")).map(|l| format!("Content label: {l}"));
    Some(Post {
        network: Network::Bluesky,
        url: web_url(&who, &uri),
        id: uri,
        cid,
        author,
        created_at: created,
        text_markdown: facets_to_markdown(&text(&record["text"]), &record["facets"]),
        content_warning: cw,
        media,
        quote,
        link_card,
        reply_to: Some(text(&record["reply"]["parent"]["uri"])).filter(|u| !u.is_empty()),
        reposted_by: None,
        reply_count: count(&v["replyCount"]),
        boost_count: count(&v["repostCount"]),
        like_count: count(&v["likeCount"]),
        mine: {
            let (like, repost) = (text(&v["viewer"]["like"]), text(&v["viewer"]["repost"]));
            Mine { liked: !like.is_empty(), boosted: !repost.is_empty(), like_record: Some(like).filter(|l| !l.is_empty()), boost_record: Some(repost).filter(|l| !l.is_empty()) }
        },
        reply_root: Some(PostRef { uri: text(&record["reply"]["root"]["uri"]), cid: text(&record["reply"]["root"]["cid"]) }).filter(|r| !r.uri.is_empty()),
        visibility: None,
    })
}

fn embed_parts(e: &Value, media: &mut Vec<Media>, quote: &mut Option<Box<Post>>, card: &mut Option<LinkCard>) {
    let ty = text(&e["$type"]);
    match ty.as_str() {
        "app.bsky.embed.images#view" => {
            for i in e["images"].as_array().into_iter().flatten() {
                let full = text(&i["fullsize"]);
                if !full.is_empty() {
                    media.push(Media { kind: MediaKind::Image, preview_url: text(&i["thumb"]), alt: text(&i["alt"]), file_name_hint: file_name_from_url(&full, "image.jpg"), url: full, blob_cid: None });
                }
            }
        }
        "app.bsky.embed.video#view" => {
            let (cid, playlist) = (text(&e["cid"]), text(&e["playlist"]));
            if !playlist.is_empty() {
                media.push(Media {
                    kind: MediaKind::Video,
                    url: playlist,
                    preview_url: text(&e["thumbnail"]),
                    alt: text(&e["alt"]),
                    file_name_hint: if cid.is_empty() { "video.mp4".into() } else { format!("{cid}.mp4") },
                    blob_cid: Some(cid).filter(|c| !c.is_empty()),
                });
            }
        }
        "app.bsky.embed.external#view" => {
            let x = &e["external"];
            if !text(&x["uri"]).is_empty() {
                *card = Some(LinkCard { url: text(&x["uri"]), title: text(&x["title"]), description: text(&x["description"]) });
            }
        }
        "app.bsky.embed.record#view" => {
            let r = &e["record"];
            if text(&r["$type"]).ends_with("#viewRecord") {
                let inner: Vec<&Value> = r["embeds"].as_array().into_iter().flatten().collect();
                *quote = build_post(r, &r["value"], &inner).map(Box::new);
            }
        }
        "app.bsky.embed.recordWithMedia#view" => {
            embed_parts(&e["media"], media, quote, card);
            embed_parts(&e["record"], media, quote, card);
        }
        _ => {}
    }
}

/// The text of a post as Markdown: links, mentions and hashtags (facets) become Markdown links.
/// Facet positions are UTF-8 BYTE offsets; facets that overlap, run past the text or cut a
/// character in two are ignored.
pub fn facets_to_markdown(text: &str, facets: &Value) -> String {
    let mut marks: Vec<(usize, usize, String)> = Vec::new();
    for f in facets.as_array().into_iter().flatten() {
        let (Some(start), Some(end)) = (f["index"]["byteStart"].as_u64(), f["index"]["byteEnd"].as_u64()) else { continue };
        let (start, end) = (start as usize, end as usize);
        if start >= end || end > text.len() || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            continue;
        }
        let shown = &text[start..end];
        let feature = f["features"].as_array().into_iter().flatten().find_map(|ft| {
            let ty = super::text(&ft["$type"]);
            if ty.ends_with("#link") {
                let uri = super::text(&ft["uri"]);
                Some(if shown == uri || shown.ends_with('…') { md_url(&uri).unwrap_or_else(|| shown.to_string()) } else { md_link(shown, &uri) })
            } else if ty.ends_with("#mention") {
                let did = super::text(&ft["did"]);
                (!did.is_empty()).then(|| md_link(shown, &format!("https://bsky.app/profile/{did}")))
            } else if ty.ends_with("#tag") {
                let tag = super::text(&ft["tag"]);
                (!tag.is_empty()).then(|| md_link(shown, &format!("https://bsky.app/hashtag/{}", url_encode(&tag))))
            } else {
                None
            }
        });
        if let Some(m) = feature {
            marks.push((start, end, m));
        }
    }
    marks.sort_by_key(|m| m.0);
    let mut out = String::new();
    let mut at = 0;
    for (s, e, m) in marks {
        if s < at {
            continue;
        }
        out.push_str(&text[at..s]);
        out.push_str(&m);
        at = e;
    }
    out.push_str(&text[at..]);
    out.trim().to_string()
}

// ---------------------------------------------------------------- the real video file

/// The server where the data of an account lives, from its DID document
/// (did:plc through plc.directory, did:web from the host's `.well-known/did.json`).
pub fn resolve_pds(did: &str) -> Result<String, String> {
    static CACHE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
    if let Some(p) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(did)) {
        return Ok(p.clone());
    }
    if !did.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '-' | '_' | '%')) {
        return Err("bad account id".into());
    }
    let url = if let Some(id) = did.strip_prefix("did:plc:") {
        if id.contains(':') || id.contains('%') {
            return Err("bad account id".into());
        }
        format!("https://plc.directory/{did}")
    } else if let Some(host) = did.strip_prefix("did:web:") {
        let host = host.replace("%3A", ":").replace("%3a", ":");
        if host.contains(':') && host.matches(':').count() > 1 || host.contains('/') || host.contains('%') {
            return Err("this kind of account address is not supported".into());
        }
        let scheme = if Origin::parse(&format!("http://{host}")).map(|o| o.is_local()).unwrap_or(false) { "http" } else { "https" };
        format!("{scheme}://{host}/.well-known/did.json")
    } else {
        return Err("unknown kind of account id".into());
    };
    let doc = api("GET", &url, None, None)?;
    let pds = pds_of_did_doc(&doc).ok_or("the account has no data server")?;
    check_url(&pds)?;
    let pds = pds.trim_end_matches('/').to_string();
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(did.to_string(), pds.clone());
    Ok(pds)
}

/// Download a blob (the original video file) from the author's own server. No login is sent.
pub fn download_blob(did: &str, cid: &str) -> Result<(Vec<u8>, Option<String>), String> {
    if cid.is_empty() || !cid.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("bad file id".into());
    }
    let pds = resolve_pds(did)?;
    download(&format!("{pds}/xrpc/com.atproto.sync.getBlob{}", query(&[("did", did), ("cid", cid)])))
}

// ---------------------------------------------------------------- writing

/// Like / repost a post, or undo it: a record in the account's repository (SOC-050). The record
/// that undoes it comes from the post (`Mine`) and is kept from the answer.
pub fn react(api: &mut Api, post: &Post, kind: Reaction, on: bool) -> Result<Reacted, ActionError> {
    let _step = tracing::info_span!("flow.social_bluesky.react", peer = "server").entered();
    let (collection, current) = match kind {
        Reaction::Like => ("app.bsky.feed.like", &post.mine.like_record),
        Reaction::Boost => ("app.bsky.feed.repost", &post.mine.boost_record),
    };
    let did = api.did()?;
    if on {
        if post.cid.is_empty() || !post.id.starts_with("at://") {
            return Err("this post cannot be reacted to (its content id is missing)".into());
        }
        let record = json!({"$type": collection, "subject": {"uri": post.id, "cid": post.cid}, "createdAt": iso_now()});
        let v = api.call("POST", "com.atproto.repo.createRecord", &[], Some(&json!({"repo": did, "collection": collection, "record": record})))?;
        let uri = text(&v["uri"]);
        if !uri.starts_with("at://") {
            return Err("the server did not name the new record".into());
        }
        return Ok(Reacted { on: true, record: Some(uri), count: None });
    }
    // undo: the record is named by its address at://<did>/<collection>/<rkey>
    let uri = current.as_deref().ok_or("Shale does not know which record to remove; reload the feed and try again")?;
    let mut parts = uri.strip_prefix("at://").unwrap_or("").splitn(3, '/');
    let (repo, coll, rkey) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if coll != collection || rkey.is_empty() || repo != did {
        return Err("the record to remove is not one of this account's".into());
    }
    api.call("POST", "com.atproto.repo.deleteRecord", &[], Some(&json!({"repo": did, "collection": collection, "rkey": rkey})))?;
    Ok(Reacted { on: false, record: None, count: None })
}

/// The facets of a post's text: links, mentions and hashtags with their UTF-8 byte offsets
/// (SOC-052). `resolve` turns a handle into a DID; a mention that does not resolve stays plain text.
pub fn facets(text: &str, mut resolve: impl FnMut(&str) -> Option<String>) -> Vec<Value> {
    let mut out = vec![];
    let mut at = 0;
    for word in text.split_inclusive(char::is_whitespace) {
        let start = at;
        at += word.len();
        let bare = word.trim_end_matches(char::is_whitespace);
        let lead = bare.len() - bare.trim_start_matches(['(', '[', '{', '"', '\'']).len();
        let tok = &bare[lead..];
        let from = start + lead;
        let trimmed = tok.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '"', '\'']);
        let index = |t: &str| json!({"byteStart": from, "byteEnd": from + t.len()});
        if (trimmed.starts_with("https://") || trimmed.starts_with("http://")) && trimmed.contains('.') && check_url(trimmed).is_ok() {
            out.push(json!({"index": index(trimmed), "features": [{"$type": "app.bsky.richtext.facet#link", "uri": trimmed}]}));
        } else if let Some(handle) = trimmed.strip_prefix('@').filter(|h| is_handle(h)) {
            if let Some(did) = resolve(handle) {
                out.push(json!({"index": index(trimmed), "features": [{"$type": "app.bsky.richtext.facet#mention", "did": did}]}));
            }
        } else if let Some(tag) = trimmed.strip_prefix('#').filter(|t| !t.is_empty() && !t.contains('#') && t.chars().count() <= 64 && !t.chars().all(|c| c.is_ascii_digit())) {
            out.push(json!({"index": index(trimmed), "features": [{"$type": "app.bsky.richtext.facet#tag", "tag": tag}]}));
        }
    }
    out
}

/// The answer to a createRecord with a record key that is taken: status 409, or words that say so.
fn is_conflict(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    m.contains("status 409") || m.contains("already exist") || m.contains("duplicate") || m.contains("conflict")
}

/// "ada.bsky.social": labels of letters, digits and hyphens, at least two.
fn is_handle(h: &str) -> bool {
    let labels: Vec<&str> = h.split('.').collect();
    labels.len() >= 2 && h.len() <= 253 && labels.iter().all(|l| !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-') && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

/// How many graphemes a Bluesky post has (the limit is 300, SOC-052).
pub fn grapheme_count(text: &str) -> usize {
    unicode_segmentation::UnicodeSegmentation::graphemes(text, true).count()
}

/// Send a post: pictures first (`uploadBlob`), then the record with facets, reply references and
/// the embed (pictures, a quote, or both). Then the post is read back from the server; when it
/// is not there yet, the post is made from what was sent.
pub fn send(api: &mut Api, d: &Draft) -> Result<Post, ActionError> {
    let _step = tracing::info_span!("flow.social_bluesky.send", peer = "server").entered();
    if grapheme_count(&d.text) > BLUESKY_MAX_CHARS || d.text.len() > 3000 {
        return Err(format!("A Bluesky post has at most {BLUESKY_MAX_CHARS} characters.").into());
    }
    if d.pictures.len() > d.info.max_pictures {
        return Err(format!("A Bluesky post has at most {} pictures.", d.info.max_pictures).into());
    }
    let did = api.did()?;
    let mut images = vec![];
    for p in &d.pictures {
        let prepared = prepare(&p.path, Network::Bluesky, BLUESKY_IMAGE_BYTES)?;
        let file = TempFile::new(&prepared.bytes, prepared.ext)?;
        let v = api.upload_blob(&file.0, prepared.mime)?;
        if !v["blob"].is_object() {
            return Err("the server did not accept the picture".into());
        }
        images.push(json!({"alt": p.alt.trim(), "image": v["blob"].clone()}));
    }
    let mut known: HashMap<String, Option<String>> = HashMap::new();
    let facets = facets(&d.text, |h| known.entry(h.to_string()).or_insert_with(|| api.get("com.atproto.identity.resolveHandle", &[("handle", h)]).ok().map(|v| text(&v["did"])).filter(|d| !d.is_empty())).clone());
    let mut record = json!({"$type": "app.bsky.feed.post", "text": d.text, "createdAt": iso_now()});
    if !facets.is_empty() {
        record["facets"] = Value::Array(facets);
    }
    if let Some(p) = &d.reply_to {
        let parent = json!({"uri": p.id, "cid": p.cid});
        let root = p.reply_root.as_ref().map(|r| json!({"uri": r.uri, "cid": r.cid})).unwrap_or_else(|| parent.clone());
        record["reply"] = json!({"root": root, "parent": parent});
    }
    let pictures = (!images.is_empty()).then(|| json!({"$type": "app.bsky.embed.images", "images": images}));
    let quote = d.quote.as_ref().map(|q| json!({"$type": "app.bsky.embed.record", "record": {"uri": q.id, "cid": q.cid}}));
    match (pictures, quote) {
        (Some(m), Some(r)) => record["embed"] = json!({"$type": "app.bsky.embed.recordWithMedia", "media": m, "record": r}),
        (Some(e), None) | (None, Some(e)) => record["embed"] = e,
        (None, None) => {}
    }
    // the record key is the key of the draft: a retry of a send that went through but did not
    // answer finds its record there already, which counts as posted (SOC-057)
    let mut body = json!({"repo": did, "collection": "app.bsky.feed.post", "record": record});
    if !d.key.is_empty() {
        body["rkey"] = json!(d.key);
    }
    let (uri, cid) = match api.call("POST", "com.atproto.repo.createRecord", &[], Some(&body)) {
        Ok(v) => (text(&v["uri"]), text(&v["cid"])),
        Err(e) if !d.key.is_empty() && is_conflict(&e) => (format!("at://{did}/app.bsky.feed.post/{}", d.key), String::new()),
        Err(e) => return Err(e.into()),
    };
    if !uri.starts_with("at://") {
        return Err("the server did not name the new post".into());
    }
    if let Ok(p) = fetch_post(api, &uri) {
        return Ok(p);
    }
    // the post is not in the feed server yet: made from what was sent
    let handle = api.session.as_ref().map(|s| s.handle.clone()).unwrap_or_else(|| api.handle.clone());
    let who = Author { id: did, handle: handle.clone(), display_name: String::new(), profile_url: format!("https://bsky.app/profile/{handle}"), avatar_url: String::new() };
    Ok(Post {
        network: Network::Bluesky,
        url: web_url(&handle, &uri),
        id: uri,
        cid,
        author: who,
        created_at: t::now(),
        text_markdown: facets_to_markdown(&d.text, &record["facets"]),
        content_warning: None,
        media: vec![],
        quote: d.quote.clone().map(|q| Box::new(Post { reposted_by: None, ..q })),
        link_card: None,
        reply_to: d.reply_to.as_ref().map(|p| p.id.clone()),
        reposted_by: None,
        reply_count: 0,
        boost_count: 0,
        like_count: 0,
        mine: Mine::default(),
        reply_root: None,
        visibility: None,
    })
}

#[cfg(test)]
mod tests {
    use super::super::actions::{DraftPicture, InstanceInfo};
    use super::*;
    use crate::calendar::dav::tests::mock_http;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn facets_with_multibyte_text() {
        // "Grüße 😀 @ada.test see example.com/very/lo… #Rust" : ü and ß are 2 bytes, the emoji 4
        let text = "Grüße 😀 @ada.test see example.com/very/lo… #Rust";
        let at = |needle: &str| {
            let s = text.find(needle).unwrap();
            (s, s + needle.len())
        };
        let (m, l, t) = (at("@ada.test"), at("example.com/very/lo…"), at("#Rust"));
        let facets = json!([
            {"index": {"byteStart": t.0, "byteEnd": t.1}, "features": [{"$type": "app.bsky.richtext.facet#tag", "tag": "Rust"}]},
            {"index": {"byteStart": m.0, "byteEnd": m.1}, "features": [{"$type": "app.bsky.richtext.facet#mention", "did": "did:plc:ada"}]},
            {"index": {"byteStart": l.0, "byteEnd": l.1}, "features": [{"$type": "app.bsky.richtext.facet#link", "uri": "https://example.com/very/long/path"}]}]);
        assert_eq!(facets_to_markdown(text, &facets), "Grüße 😀 [@ada.test](https://bsky.app/profile/did:plc:ada) see https://example.com/very/long/path [#Rust](https://bsky.app/hashtag/Rust)");
        // a link whose text is not the address keeps its text
        let f = json!([{"index": {"byteStart": 0, "byteEnd": 7}, "features": [{"$type": "app.bsky.richtext.facet#link", "uri": "https://x.org/ü"}]}]);
        assert_eq!(facets_to_markdown("Grüße!", &f), "[Grüße](https://x.org/ü)!");
        // bad facets are ignored instead of cutting a character or panicking: inside "ü" (byte 3), past the end, backwards, overlapping
        let link = json!([{"$type": "app.bsky.richtext.facet#link", "uri": "https://x.org/"}]);
        let bad = json!([
            {"index": {"byteStart": 3, "byteEnd": 4}, "features": link},
            {"index": {"byteStart": 0, "byteEnd": 99}, "features": link},
            {"index": {"byteStart": 4, "byteEnd": 2}, "features": link},
            {"index": {"byteStart": 0, "byteEnd": 4}, "features": link},
            {"index": {"byteStart": 2, "byteEnd": 5}, "features": link},
            {"index": {"byteStart": "x"}, "features": link}]);
        assert_eq!(facets_to_markdown("Grüße", &bad), "[Grü](https://x.org/)ße");
        assert_eq!(facets_to_markdown("  plain  ", &Value::Null), "plain");
    }

    pub(crate) fn post_view(n: &str, did: &str, handle: &str, at: &str, reply_to: Option<&str>, txt: &str) -> Value {
        let uri = format!("at://{did}/app.bsky.feed.post/{n}");
        let mut record = json!({"$type": "app.bsky.feed.post", "text": txt, "createdAt": at});
        if let Some(r) = reply_to {
            record["reply"] = json!({"parent": {"uri": if r.starts_with("at://") { r.to_string() } else { format!("at://{did}/app.bsky.feed.post/{r}") }, "cid": "c"}, "root": {"uri": "at://x/y/z", "cid": "c"}});
        }
        json!({"uri": uri, "cid": format!("cid{n}"), "author": {"did": did, "handle": handle, "displayName": handle.to_uppercase(), "avatar": "https://cdn.test/av.jpg"}, "record": record, "replyCount": 1, "repostCount": 2, "likeCount": 3, "indexedAt": at})
    }

    /// A pretend server with a session, for the tests of writes: `handler(nsid, body)` answers the rest.
    fn writing_server(n: usize, handler: impl Fn(&str, &str, &Value) -> (u16, String) + Send + 'static) -> (String, std::thread::JoinHandle<Vec<String>>) {
        mock_http(n + 1, move |method, path, headers, body| {
            let nsid = path.split('?').next().unwrap().trim_start_matches("/xrpc/").to_string();
            if nsid == "com.atproto.server.createSession" {
                let host = headers.lines().find_map(|l| l.strip_prefix("Host: ")).unwrap_or("").trim().to_string();
                return (200, json!({"accessJwt": "jwt-1", "refreshJwt": "ref-1", "did": "did:plc:ada", "handle": "ada.test", "didDoc": {"service": [{"id": "#atproto_pds", "serviceEndpoint": format!("http://{host}")}]}}).to_string());
            }
            assert!(headers.to_ascii_lowercase().contains("authorization: bearer jwt-1"), "{nsid}: {headers}");
            let json: Value = serde_json::from_str(body).unwrap_or(Value::Null);
            handler(method, &nsid, &json)
        })
    }

    // spec: SOC-050
    #[test]
    fn a_post_view_tells_what_the_account_did_with_it_and_the_records_that_undo_it() {
        let mut v = post_view("1", "did:plc:bob", "bob.test", "2024-05-17T10:30:00Z", Some("2"), "hi");
        assert_eq!(parse_post_view(&v).unwrap().mine, Mine::default());
        v["viewer"] = json!({"like": "at://did:plc:ada/app.bsky.feed.like/3k1", "repost": "at://did:plc:ada/app.bsky.feed.repost/3k2"});
        let p = parse_post_view(&v).unwrap();
        assert_eq!(p.mine, Mine { liked: true, boosted: true, like_record: Some("at://did:plc:ada/app.bsky.feed.like/3k1".into()), boost_record: Some("at://did:plc:ada/app.bsky.feed.repost/3k2".into()) });
        // the first post of the conversation is kept to answer it (SOC-051)
        assert_eq!(p.reply_root, Some(PostRef { uri: "at://x/y/z".into(), cid: "c".into() }));
    }

    // spec: SOC-060
    #[test]
    fn a_quoted_record_keeps_its_uri_cid_counts_and_address_and_can_be_liked_and_reposted_by_them() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let mut v = post_view("1", "did:plc:ada", "ada.test", "2024-05-17T10:30:00Z", None, "look");
        v["embed"] = json!({"$type": "app.bsky.embed.record#view", "record": {"$type": "app.bsky.embed.record#viewRecord", "uri": "at://did:plc:bob/app.bsky.feed.post/q1", "cid": "cq",
            "author": {"did": "did:plc:bob", "handle": "bob.test"}, "value": {"text": "quoted", "createdAt": "2024-05-16T10:30:00Z", "reply": {"parent": {"uri": "at://did:plc:bob/app.bsky.feed.post/q0", "cid": "c0"}, "root": {"uri": "at://did:plc:bob/app.bsky.feed.post/q0", "cid": "c0"}}},
            "replyCount": 4, "repostCount": 5, "likeCount": 6}});
        let q = *parse_post_view(&v).unwrap().quote.unwrap();
        assert_eq!((q.id.as_str(), q.cid.as_str(), q.url.as_str(), q.reply_to.as_deref()), ("at://did:plc:bob/app.bsky.feed.post/q1", "cq", "https://bsky.app/profile/bob.test/post/q1", Some("at://did:plc:bob/app.bsky.feed.post/q0")));
        assert_eq!((q.reply_count, q.boost_count, q.like_count), (4, 5, 6));
        assert_eq!(q.mine, Mine::default(), "a quoted record carries no viewer state: nothing is liked until the account does it");
        let (base, handle) = writing_server(2, |_, nsid, body| {
            assert_eq!(nsid, "com.atproto.repo.createRecord");
            assert_eq!((body["record"]["subject"]["uri"].as_str(), body["record"]["subject"]["cid"].as_str()), (Some("at://did:plc:bob/app.bsky.feed.post/q1"), Some("cq")), "the quote, not the post that quotes");
            let coll = body["collection"].as_str().unwrap();
            (200, json!({"uri": format!("at://did:plc:ada/{coll}/3q"), "cid": "n"}).to_string())
        });
        let mut api = Api::new(&base, "ada.test", "app-pw");
        assert_eq!(react(&mut api, &q, Reaction::Like, true).unwrap().record.as_deref(), Some("at://did:plc:ada/app.bsky.feed.like/3q"));
        assert_eq!(react(&mut api, &q, Reaction::Boost, true).unwrap().record.as_deref(), Some("at://did:plc:ada/app.bsky.feed.repost/3q"));
        assert_eq!(handle.join().unwrap().len(), 3);
    }

    // spec: SOC-050
    #[test]
    fn like_and_repost_are_records_that_are_created_and_deleted_against_a_mock_server() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let (base, handle) = writing_server(3, |method, nsid, body| {
            assert_eq!(method, "POST");
            match nsid {
                "com.atproto.repo.createRecord" => {
                    let coll = body["collection"].as_str().unwrap().to_string();
                    assert_eq!(
                        (body["repo"].as_str(), body["record"]["$type"].as_str(), body["record"]["subject"]["uri"].as_str(), body["record"]["subject"]["cid"].as_str()),
                        (Some("did:plc:ada"), Some(coll.as_str()), Some("at://did:plc:bob/app.bsky.feed.post/1"), Some("cid1"))
                    );
                    assert!(body["record"]["createdAt"].as_str().unwrap().ends_with('Z'));
                    (200, json!({"uri": format!("at://did:plc:ada/{coll}/3k9"), "cid": "newcid"}).to_string())
                }
                "com.atproto.repo.deleteRecord" => {
                    assert_eq!((body["repo"].as_str(), body["collection"].as_str(), body["rkey"].as_str()), (Some("did:plc:ada"), Some("app.bsky.feed.like"), Some("3k1")));
                    (200, "{}".into())
                }
                _ => (404, json!({"error": "NotFound"}).to_string()),
            }
        });
        let mut api = Api::new(&base, "ada.test", "app-pw");
        let mut p = parse_post_view(&post_view("1", "did:plc:bob", "bob.test", "2024-05-17T10:30:00Z", None, "hi")).unwrap();
        let liked = react(&mut api, &p, Reaction::Like, true).unwrap();
        assert_eq!(liked, Reacted { on: true, record: Some("at://did:plc:ada/app.bsky.feed.like/3k9".into()), count: None });
        let reposted = react(&mut api, &p, Reaction::Boost, true).unwrap();
        assert_eq!(reposted.record.as_deref(), Some("at://did:plc:ada/app.bsky.feed.repost/3k9"));
        // undo: the record that was made (or that the post view named) is deleted
        p.mine = Mine { liked: true, like_record: Some("at://did:plc:ada/app.bsky.feed.like/3k1".into()), ..Mine::default() };
        assert_eq!(react(&mut api, &p, Reaction::Like, false).unwrap(), Reacted { on: false, record: None, count: None });
        // no record known, or somebody else's record: nothing is sent
        assert!(react(&mut api, &Post { mine: Mine::default(), ..p.clone() }, Reaction::Boost, false).unwrap_err().to_string().contains("which record"));
        let alien = Post { mine: Mine { boosted: true, boost_record: Some("at://did:plc:eve/app.bsky.feed.repost/1".into()), ..Mine::default() }, ..p.clone() };
        assert!(react(&mut api, &alien, Reaction::Boost, false).unwrap_err().to_string().contains("not one of this account's"));
        assert!(react(&mut api, &Post { cid: String::new(), ..p }, Reaction::Like, true).is_err(), "without the content id of the post");
        assert_eq!(handle.join().unwrap().len(), 4, "the login and three requests");
    }

    fn draft(text: &str) -> Draft {
        Draft { text: text.into(), cw: String::new(), visibility: super::super::actions::Visibility::Public, reply_to: None, quote: None, pictures: vec![], info: InstanceInfo::defaults(Network::Bluesky), key: "keybsky00000a".into() }
    }

    // spec: SOC-051, SOC-052, SOC-054
    #[test]
    fn a_post_is_a_record_with_facets_the_reply_references_and_the_quote_embed_against_a_mock_server() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let seen2 = seen.clone();
        // the fourth post is not in the feed server yet when it is read back
        let (base, handle) = writing_server(9, move |method, nsid, body| match (method, nsid) {
            ("GET", "com.atproto.identity.resolveHandle") => (200, json!({"did": "did:plc:bob"}).to_string()),
            ("POST", "com.atproto.repo.createRecord") => {
                assert_eq!((body["repo"].as_str(), body["collection"].as_str()), (Some("did:plc:ada"), Some("app.bsky.feed.post")));
                seen2.lock().unwrap().push(body["record"].clone());
                let n = seen2.lock().unwrap().len();
                (200, json!({"uri": format!("at://did:plc:ada/app.bsky.feed.post/n{n}"), "cid": format!("cn{n}")}).to_string())
            }
            ("GET", "app.bsky.feed.getPosts") => {
                let n = seen2.lock().unwrap().len();
                if n == 4 {
                    return (200, json!({"posts": []}).to_string());
                }
                let record = seen2.lock().unwrap()[n - 1].clone();
                let mut v = post_view(&format!("n{n}"), "did:plc:ada", "ada.test", "2024-05-17T10:30:00Z", None, record["text"].as_str().unwrap());
                v["record"] = record;
                (200, json!({"posts": [v]}).to_string())
            }
            _ => (404, json!({"error": "NotFound"}).to_string()),
        });
        let mut api = Api::new(&base, "ada.test", "app-pw");
        let parent = parse_post_view(&post_view("1", "did:plc:bob", "bob.test", "2024-05-17T10:30:00Z", Some("0"), "q")).unwrap();
        let top = Post { reply_root: None, reply_to: None, ..parent.clone() };
        // 1: a reply inside a conversation: the root of the conversation is kept, the parent is the post
        let one = send(&mut api, &Draft { reply_to: Some(parent.clone()), ..draft("hi @bob.test see https://e.com/x #tag") }).unwrap();
        let r = seen.lock().unwrap()[0].clone();
        assert_eq!(r["$type"], "app.bsky.feed.post");
        assert_eq!(r["text"], "hi @bob.test see https://e.com/x #tag");
        assert_eq!(r["reply"], json!({"root": {"uri": "at://x/y/z", "cid": "c"}, "parent": {"uri": parent.id, "cid": "cid1"}}));
        let kinds: Vec<&str> = r["facets"].as_array().unwrap().iter().map(|f| f["features"][0]["$type"].as_str().unwrap().rsplit('#').next().unwrap()).collect();
        assert_eq!(kinds, ["mention", "link", "tag"]);
        assert_eq!((r["facets"][0]["index"]["byteStart"].as_u64(), r["facets"][0]["features"][0]["did"].as_str()), (Some(3), Some("did:plc:bob")));
        assert!(r["createdAt"].as_str().unwrap().ends_with('Z'));
        assert_eq!((one.id.as_str(), one.cid.as_str()), ("at://did:plc:ada/app.bsky.feed.post/n1", "cidn1"));
        // 2: a reply to a post that starts a conversation: it is its own root
        send(&mut api, &Draft { reply_to: Some(top.clone()), ..draft("second") }).unwrap();
        assert_eq!(seen.lock().unwrap()[1]["reply"]["root"], json!({"uri": top.id, "cid": "cid1"}));
        assert!(seen.lock().unwrap()[1].get("facets").is_none() && seen.lock().unwrap()[1].get("embed").is_none());
        // 3: a quote is an embedded record
        send(&mut api, &Draft { quote: Some(top.clone()), ..draft("look at this") }).unwrap();
        assert_eq!(seen.lock().unwrap()[2]["embed"], json!({"$type": "app.bsky.embed.record", "record": {"uri": top.id, "cid": "cid1"}}));
        // 4: not in the feed server yet: the post is made from what was sent
        let local = send(&mut api, &Draft { quote: Some(top.clone()), reply_to: Some(parent.clone()), ..draft("see https://e.com") }).unwrap();
        assert_eq!(
            (local.id.as_str(), local.author.handle.as_str(), local.reply_to.as_deref(), local.quote.as_ref().map(|q| q.id.clone())),
            ("at://did:plc:ada/app.bsky.feed.post/n4", "ada.test", Some(parent.id.as_str()), Some(top.id.clone()))
        );
        assert!(local.text_markdown.contains("https://e.com"), "{}", local.text_markdown);
        // too long: refused before anything is sent
        let long = "x".repeat(301);
        assert!(send(&mut api, &draft(&long)).unwrap_err().to_string().contains("at most 300"));
        let calls = handle.join().unwrap();
        assert_eq!(calls.iter().filter(|c| c.contains("createRecord")).count(), 4, "{calls:?}");
    }

    // spec: SOC-057
    #[test]
    fn the_record_key_is_the_key_of_the_draft_and_a_taken_key_on_a_retry_counts_as_posted() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let taken = Arc::new(AtomicBool::new(false));
        let keys = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let (taken2, keys2) = (taken.clone(), keys.clone());
        let (base, handle) = writing_server(4, move |method, nsid, body| match (method, nsid) {
            ("POST", "com.atproto.repo.createRecord") => {
                let key = body["rkey"].as_str().unwrap_or("").to_string();
                keys2.lock().unwrap().push(key.clone());
                // the first try made the record but the answer was lost; the retry meets the record
                if key == "keybsky00000a" && taken2.swap(true, Ordering::SeqCst) {
                    return (400, json!({"error": "InvalidRequest", "message": "Record already exists"}).to_string());
                }
                (200, json!({"uri": format!("at://did:plc:ada/app.bsky.feed.post/{key}"), "cid": "c1"}).to_string())
            }
            ("GET", "app.bsky.feed.getPosts") => (200, json!({"posts": [post_view("keybsky00000a", "did:plc:ada", "ada.test", "2024-05-17T10:30:00Z", None, "same")]}).to_string()),
            _ => (404, json!({"error": "NotFound"}).to_string()),
        });
        let mut api = Api::new(&base, "ada.test", "app-pw");
        let first = send(&mut api, &draft("same")).unwrap();
        let retry = send(&mut api, &draft("same")).unwrap();
        assert_eq!((first.id.as_str(), retry.id.as_str()), ("at://did:plc:ada/app.bsky.feed.post/keybsky00000a", "at://did:plc:ada/app.bsky.feed.post/keybsky00000a"), "the retry is the post, read back");
        assert_eq!(*keys.lock().unwrap(), ["keybsky00000a", "keybsky00000a"], "the same record key");
        // other refusals are errors, not posted
        assert!(is_conflict("the server answered with status 409") && !is_conflict("the server answered with status 500: Internal error"));
        handle.join().unwrap();
    }

    // spec: SOC-053, SOC-054
    #[test]
    fn pictures_are_blobs_with_alt_text_in_an_images_embed_also_beside_a_quote_against_a_mock_server() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let small = crate::social::actions::tests::file("bsky-small.png", &crate::social::actions::tests::png(30, 20, false));
        // too big for Bluesky: it is made smaller before it goes up
        let big = crate::social::actions::tests::file("bsky-big.png", &crate::social::actions::tests::png(700, 700, true));
        let uploads = Arc::new(std::sync::Mutex::new(Vec::<(String, usize)>::new()));
        let (up2, records) = (uploads.clone(), Arc::new(std::sync::Mutex::new(Vec::<Value>::new())));
        let rec2 = records.clone();
        let (base, handle) = mock_http(8, move |method, path, headers, body| {
            let nsid = path.trim_start_matches("/xrpc/").to_string();
            let low = headers.to_ascii_lowercase();
            match (method, nsid.as_str()) {
                ("POST", "com.atproto.server.createSession") => {
                    let host = headers.lines().find_map(|l| l.strip_prefix("Host: ")).unwrap_or("").trim().to_string();
                    (200, json!({"accessJwt": "jwt-1", "refreshJwt": "ref-1", "did": "did:plc:ada", "handle": "ada.test", "didDoc": {"service": [{"id": "#atproto_pds", "serviceEndpoint": format!("http://{host}")}]}}).to_string())
                }
                ("POST", "com.atproto.repo.uploadBlob") => {
                    assert!(low.contains("authorization: bearer jwt-1"));
                    let ct = low.lines().find_map(|l| l.strip_prefix("content-type: ")).unwrap_or("").trim().to_string();
                    let n = up2.lock().unwrap().len();
                    // (the mock reads the body as text: the real size is the Content-Length)
                    let len: usize = low.lines().find_map(|l| l.strip_prefix("content-length: ")).and_then(|v| v.trim().parse().ok()).unwrap();
                    up2.lock().unwrap().push((ct.clone(), len));
                    (200, json!({"blob": {"$type": "blob", "ref": {"$link": format!("bafy{n}")}, "mimeType": ct, "size": len}}).to_string())
                }
                ("POST", "com.atproto.repo.createRecord") => {
                    rec2.lock().unwrap().push(serde_json::from_str::<Value>(body).unwrap()["record"].clone());
                    (200, json!({"uri": "at://did:plc:ada/app.bsky.feed.post/p1", "cid": "cp1"}).to_string())
                }
                ("GET", "app.bsky.feed.getPosts") => (200, json!({"posts": []}).to_string()),
                _ => (404, json!({"error": "NotFound"}).to_string()),
            }
        });
        let mut api = Api::new(&base, "ada.test", "app-pw");
        let quoted = parse_post_view(&post_view("9", "did:plc:bob", "bob.test", "2024-05-17T10:30:00Z", None, "q")).unwrap();
        let pictures = vec![DraftPicture { path: small, alt: " A small one ".into() }, DraftPicture { path: big, alt: String::new() }];
        send(&mut api, &Draft { pictures: pictures.clone(), ..draft("with pictures") }).unwrap();
        let r = records.lock().unwrap()[0].clone();
        assert_eq!(r["embed"]["$type"], "app.bsky.embed.images");
        assert_eq!((r["embed"]["images"][0]["alt"].as_str(), r["embed"]["images"][1]["alt"].as_str()), (Some("A small one"), Some("")));
        assert_eq!(r["embed"]["images"][0]["image"]["ref"]["$link"], "bafy0", "the blob the server named");
        let ups = uploads.lock().unwrap().clone();
        assert_eq!((ups[0].0.as_str(), ups[1].0.as_str()), ("image/png", "image/jpeg"), "the big one became a JPEG");
        assert!(ups[1].1 as u64 <= BLUESKY_IMAGE_BYTES, "{}", ups[1].1);
        // pictures and a quote: recordWithMedia
        send(&mut api, &Draft { pictures: pictures[..1].to_vec(), quote: Some(quoted.clone()), ..draft("both") }).unwrap();
        let r = records.lock().unwrap()[1].clone();
        assert_eq!(r["embed"]["$type"], "app.bsky.embed.recordWithMedia");
        assert_eq!((r["embed"]["media"]["$type"].as_str(), r["embed"]["record"]["record"]["uri"].as_str(), r["embed"]["record"]["$type"].as_str()), (Some("app.bsky.embed.images"), Some(quoted.id.as_str()), Some("app.bsky.embed.record")));
        // nothing is uploaded for a refused picture
        let nothing = crate::social::actions::tests::file("bsky-text.png", b"words");
        let before = uploads.lock().unwrap().len();
        let e = send(&mut api, &Draft { pictures: vec![DraftPicture { path: nothing, alt: String::new() }], ..draft("x") }).unwrap_err();
        assert!(e.to_string().contains("not a JPEG, PNG, GIF or WebP"), "{e}");
        assert_eq!(uploads.lock().unwrap().len(), before);
        // too many pictures
        let five = vec![pictures[0].clone(); 5];
        assert!(send(&mut api, &Draft { pictures: five, ..draft("x") }).unwrap_err().to_string().contains("at most 4 pictures"));
        assert_eq!(handle.join().unwrap().len(), 8, "the login, 3 uploads, 2 records and 2 reads");
    }

    // spec: SOC-051, SOC-052
    #[test]
    fn links_mentions_and_tags_become_facets_with_utf8_byte_offsets() {
        let text = "Grüße 😀 @ada.test see (https://e.com/x?a=1). #Rust, @nobody.test and #123 @bad";
        let mut asked = vec![];
        let f = facets(text, |h| {
            asked.push(h.to_string());
            (h == "ada.test").then(|| "did:plc:ada".to_string())
        });
        assert_eq!(asked, ["ada.test", "nobody.test"], "a handle needs a dot; a number is no tag");
        let slice = |v: &Value| &text.as_bytes()[v["index"]["byteStart"].as_u64().unwrap() as usize..v["index"]["byteEnd"].as_u64().unwrap() as usize];
        let kinds: Vec<(&str, String)> = f.iter().map(|v| (v["features"][0]["$type"].as_str().unwrap().rsplit('#').next().unwrap(), String::from_utf8(slice(v).to_vec()).unwrap())).collect();
        // the "(" in front and the ")." behind a link are not part of it; the unresolved mention is plain text
        assert_eq!(kinds, [("mention", "@ada.test".to_string()), ("link", "https://e.com/x?a=1".to_string()), ("tag", "#Rust".to_string())]);
        assert_eq!((f[0]["features"][0]["did"].as_str(), f[1]["features"][0]["uri"].as_str(), f[2]["features"][0]["tag"].as_str()), (Some("did:plc:ada"), Some("https://e.com/x?a=1"), Some("Rust")));
        // the facets read back as Markdown links (what a feed shows)
        let md = facets_to_markdown(text, &Value::Array(f));
        assert!(md.contains("[@ada.test](https://bsky.app/profile/did:plc:ada)") && md.contains("(https://e.com/x?a=1)"), "{md}");
        assert!(facets("no facets here, not even e.com or @ or #", |_| None).is_empty());
        assert_eq!(grapheme_count("a😀e\u{301}👨‍👩‍👧"), 4, "an emoji with a joiner and a letter with an accent count once");
    }

    #[test]
    fn embeds_become_media_cards_and_quotes() {
        let mut v = post_view("1", "did:plc:ada", "ada.test", "2024-05-17T10:30:00.000Z", None, "look");
        v["embed"] = json!({"$type": "app.bsky.embed.recordWithMedia#view",
            "media": {"$type": "app.bsky.embed.images#view", "images": [{"thumb": "https://cdn.test/t/did:plc:ada/bafy1@jpeg", "fullsize": "https://cdn.test/f/did:plc:ada/bafy1@jpeg", "alt": "a cat"}]},
            "record": {"$type": "app.bsky.embed.record#view", "record": {"$type": "app.bsky.embed.record#viewRecord", "uri": "at://did:plc:bob/app.bsky.feed.post/q1", "cid": "cq", "author": {"did": "did:plc:bob", "handle": "bob.test", "displayName": "Bob"}, "value": {"text": "quoted", "createdAt": "2024-05-16T10:30:00Z"}, "embeds": [{"$type": "app.bsky.embed.external#view", "external": {"uri": "https://e.com", "title": "E", "description": "d"}}]}}});
        let p = parse_post_view(&v).unwrap();
        assert_eq!((p.id.as_str(), p.url.as_str(), p.created_at), ("at://did:plc:ada/app.bsky.feed.post/1", "https://bsky.app/profile/ada.test/post/1", 1_715_941_800));
        assert_eq!((p.media[0].kind, p.media[0].alt.as_str(), p.media[0].file_name_hint.as_str()), (MediaKind::Image, "a cat", "bafy1.jpeg"));
        let q = p.quote.unwrap();
        assert_eq!((q.text_markdown.as_str(), q.author.handle.as_str(), q.url.as_str()), ("quoted", "bob.test", "https://bsky.app/profile/bob.test/post/q1"));
        assert_eq!(q.link_card.unwrap().url, "https://e.com");
        // video: the playlist is the url, the blob the real file
        let mut v = post_view("2", "did:plc:ada", "ada.test", "2024-05-17T10:30:00Z", None, "");
        v["embed"] = json!({"$type": "app.bsky.embed.video#view", "cid": "bafyvideo", "playlist": "https://video.test/p.m3u8", "thumbnail": "https://video.test/t.jpg", "alt": "clip"});
        let p = parse_post_view(&v).unwrap();
        assert_eq!((p.media[0].kind, p.media[0].blob_cid.as_deref(), p.media[0].file_name_hint.as_str()), (MediaKind::Video, Some("bafyvideo"), "bafyvideo.mp4"));
        // a blocked post is no post
        assert!(parse_post_view(&json!({"$type": "app.bsky.feed.defs#blockedPost", "uri": "at://x/y/z"})).is_none());
        // an invalid handle falls back to the DID in addresses
        let v = post_view("3", "did:plc:zed", "handle.invalid", "2024-05-17T10:30:00Z", None, "x");
        assert_eq!(parse_post_view(&v).unwrap().url, "https://bsky.app/profile/did:plc:zed/post/3");
    }

    #[test]
    fn reposts_and_feed_items() {
        let item =
            json!({"post": post_view("1", "did:plc:ada", "ada.test", "2024-05-17T10:30:00Z", None, "hi"), "reason": {"$type": "app.bsky.feed.defs#reasonRepost", "by": {"did": "did:plc:cy", "handle": "cy.test", "displayName": "Cy"}}});
        let p = parse_feed_item(&item).unwrap();
        assert_eq!(p.reposted_by.unwrap().handle, "cy.test");
        assert!(parse_feed_item(&json!({"post": post_view("1", "did:plc:ada", "ada.test", "2024-05-17T10:30:00Z", None, "hi")})).unwrap().reposted_by.is_none());
    }

    #[test]
    fn saved_feeds_from_preferences() {
        let g = |n: &str| format!("at://did:plc:x/app.bsky.feed.generator/{n}");
        let v2 = json!({"preferences": [{"$type": "app.bsky.actor.defs#adultContentPref", "enabled": true}, {"$type": "app.bsky.actor.defs#savedFeedsPrefV2", "items": [
            {"type": "timeline", "value": "following", "pinned": true}, {"type": "feed", "value": g("b"), "pinned": false}, {"type": "feed", "value": g("a"), "pinned": true}, {"type": "list", "value": "at://did:plc:x/app.bsky.graph.list/l", "pinned": true}, {"type": "feed", "value": g("a"), "pinned": true}]}]});
        assert_eq!(saved_feed_uris(&v2), [g("a"), g("b")]);
        let old = json!({"preferences": [{"$type": "app.bsky.actor.defs#savedFeedsPref", "pinned": [g("p")], "saved": [g("p"), g("s")]}]});
        assert_eq!(saved_feed_uris(&old), [g("p"), g("s")]);
        assert!(saved_feed_uris(&json!({})).is_empty());
    }

    #[test]
    fn thread_from_a_fixture() {
        let d = "did:plc:ada";
        let t = "2024-05-17T10:0";
        let reply = |n: &str, who: (&str, &str), min: u32, parent: &str, kids: Vec<Value>| json!({"$type": "app.bsky.feed.defs#threadViewPost", "post": post_view(n, who.0, who.1, &format!("{t}{min}:00Z"), Some(parent), &format!("post {n}")), "replies": kids});
        let resp = json!({"thread": {"$type": "app.bsky.feed.defs#threadViewPost", "post": post_view("p2", d, "ada.test", &format!("{t}2:00Z"), Some("p1"), "post p2"),
            "parent": {"$type": "app.bsky.feed.defs#threadViewPost", "post": post_view("p1", d, "ada.test", &format!("{t}1:00Z"), Some("b0"), "post p1"),
                "parent": {"$type": "app.bsky.feed.defs#threadViewPost", "post": post_view("b0", "did:plc:bob", "bob.test", &format!("{t}0:00Z"), None, "post b0")}},
            "replies": [
                reply("c1", ("did:plc:cy", "cy.test"), 3, "at://did:plc:ada/app.bsky.feed.post/p2", vec![reply("a9", (d, "ada.test"), 9, "at://did:plc:cy/app.bsky.feed.post/c1", vec![])]),
                reply("p3", (d, "ada.test"), 4, "p2", vec![reply("p4", (d, "ada.test"), 5, "p3", vec![]), reply("c2", ("did:plc:cy", "cy.test"), 6, "at://did:plc:ada/app.bsky.feed.post/p3", vec![])]),
                {"$type": "app.bsky.feed.defs#notFoundPost", "uri": "at://x/y/z", "notFound": true}]}});
        let root = parse_post_view(&resp["thread"]["post"]).unwrap();
        let th = thread_from_response(&root, &resp);
        assert_eq!(th.iter().map(|p| p.text_markdown.as_str()).collect::<Vec<_>>(), ["post p1", "post p2", "post p3", "post p4"]);
        // spec: SOC-019: cy's replies (also under the author's p3), and the author's answer to cy
        let r = replies_from_response(&th, &resp);
        assert_eq!(r.iter().map(|r| (r.post.text_markdown.as_str(), r.depth)).collect::<Vec<_>>(), [("post c1", 0), ("post a9", 1), ("post c2", 0)]);
    }

    #[test]
    fn sessions_do_not_show_their_tokens() {
        let s = Session { pds: "https://p".into(), did: "did:plc:a".into(), handle: "a".into(), access_jwt: "jwt-secret-1".into(), refresh_jwt: "jwt-secret-2".into() };
        let api = Api { server: "https://bsky.social".into(), handle: "a".into(), password: "pw-secret-3".into(), session: Some(s) };
        let d = format!("{api:?}");
        assert!(!d.contains("secret") && d.contains("***"), "{d}");
        let doc = json!({"service": [{"id": "#other", "serviceEndpoint": "https://no"}, {"id": "#atproto_pds", "type": "AtprotoPersonalDataServer", "serviceEndpoint": "https://pds.example"}]});
        assert_eq!(pds_of_did_doc(&doc).as_deref(), Some("https://pds.example"));
        assert!(resolve_pds("did:plc:a/../b").is_err() && resolve_pds("did:key:abc").is_err() && resolve_pds("did:web:evil.com/x").is_err());
    }

    #[test]
    fn sign_in_expiry_feeds_paging_thread_and_media_against_a_mock_server() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let expired_once = Arc::new(AtomicBool::new(false));
        let (base, handle) = mock_http(13, move |method, path, headers, body| {
            let host = headers.lines().find_map(|l| l.strip_prefix("Host: ")).unwrap_or("").trim().to_string();
            let did = format!("did:web:{}", host.replace(':', "%3A"));
            let low = headers.to_ascii_lowercase();
            let bearer = |t: &str| low.contains(&format!("authorization: bearer {t}"));
            let ok = |v: Value| (200, v.to_string());
            let expired = || (400, json!({"error": "ExpiredToken", "message": "Token has expired"}).to_string());
            let mut img = post_view("p1", &did, "ada.test", "2024-05-17T10:00:00Z", None, "Grüße 😀 https://e.com/x");
            img["record"]["facets"] = json!([{"index": {"byteStart": 13, "byteEnd": 28}, "features": [{"$type": "app.bsky.richtext.facet#link", "uri": "https://e.com/x"}]}]);
            img["embed"] = json!({"$type": "app.bsky.embed.images#view", "images": [{"thumb": format!("http://{host}/img/t.jpg"), "fullsize": format!("http://{host}/img/f/bafyimg@jpeg"), "alt": "pic"}]});
            let mut vid = post_view("p2", &did, "ada.test", "2024-05-17T10:01:00Z", Some("p1"), "part two");
            vid["embed"] = json!({"$type": "app.bsky.embed.video#view", "cid": "bafyvid", "playlist": format!("http://{host}/vid.m3u8")});
            match (method, path.split('?').next().unwrap()) {
                ("POST", "/xrpc/com.atproto.server.createSession") => {
                    assert!(body.contains("app-pw") && !low.contains("authorization"), "{body}");
                    ok(json!({"accessJwt": "jwt-1", "refreshJwt": "ref-1", "did": did, "handle": "ada.test", "didDoc": {"service": [{"id": "#atproto_pds", "serviceEndpoint": format!("http://{host}")}]}}))
                }
                ("POST", "/xrpc/com.atproto.server.refreshSession") if bearer("ref-1") => ok(json!({"accessJwt": "jwt-2", "refreshJwt": "ref-2", "did": did, "handle": "ada.test"})),
                ("GET", "/xrpc/app.bsky.actor.getPreferences") if bearer("jwt-1") => {
                    ok(json!({"preferences": [{"$type": "app.bsky.actor.defs#savedFeedsPrefV2", "items": [{"type": "feed", "value": "at://did:plc:x/app.bsky.feed.generator/cats", "pinned": true}]}]}))
                }
                ("GET", "/xrpc/app.bsky.feed.getFeedGenerators") => ok(json!({"feeds": [{"uri": "at://did:plc:x/app.bsky.feed.generator/cats", "displayName": "Cats"}]})),
                ("GET", "/xrpc/app.bsky.feed.getPosts") if path.contains("p1") => ok(json!({"posts": [img.clone()]})),
                ("GET", "/xrpc/app.bsky.feed.getPosts") => ok(json!({"posts": []})),
                ("GET", "/xrpc/app.bsky.feed.getTimeline") => {
                    if bearer("jwt-1") && !expired_once.swap(true, Ordering::SeqCst) {
                        return expired();
                    }
                    assert!(bearer("jwt-2"), "{headers}");
                    if path.contains("cursor=c1") {
                        ok(json!({"feed": []}))
                    } else {
                        ok(json!({"feed": [{"post": img.clone(), "reason": {"$type": "app.bsky.feed.defs#reasonRepost", "by": {"did": "did:plc:cy", "handle": "cy.test"}}}], "cursor": "c1"}))
                    }
                }
                ("GET", "/xrpc/app.bsky.feed.getPostThread") => {
                    assert!(path.contains("depth=10") && path.contains("parentHeight=80") && path.contains("uri=at%3A%2F%2Fdid%3Aweb%3A"), "{path}");
                    ok(json!({"thread": {"$type": "app.bsky.feed.defs#threadViewPost", "post": img.clone(), "replies": [{"$type": "app.bsky.feed.defs#threadViewPost", "post": vid.clone(), "replies": []}]}}))
                }
                ("GET", "/.well-known/did.json") => ok(json!({"id": did, "service": [{"id": "#atproto_pds", "serviceEndpoint": format!("http://{host}")}]})),
                ("GET", "/xrpc/com.atproto.sync.getBlob") => {
                    assert!(path.contains("cid=bafyvid") && !low.contains("authorization"), "{path} {headers}");
                    (200, "VIDEODATA".into())
                }
                ("GET", "/img/f/bafyimg@jpeg") => {
                    assert!(!low.contains("authorization"), "media gets no login");
                    (200, "JPEGDATA".into())
                }
                _ => (404, json!({"error": "NotFound"}).to_string()),
            }
        });
        let mut c = Client::new(Account::new(Network::Bluesky, &base, "ada.test", "Ada"), "app-pw");
        let feeds = c.feeds().unwrap();
        assert_eq!(feeds.iter().map(|f| f.title.as_str()).collect::<Vec<_>>(), ["Following", "Cats"]);
        // the first timeline call finds the token expired, refreshes and tries again
        let page = c.timeline(&feeds[0], None).unwrap();
        assert_eq!(page.posts.len(), 1);
        assert_eq!(page.cursor.as_deref(), Some("c1"));
        let p = &page.posts[0];
        assert_eq!((p.text_markdown.as_str(), p.reposted_by.as_ref().unwrap().handle.as_str()), ("Grüße 😀 https://e.com/x", "cy.test"));
        let next = c.timeline(&feeds[0], page.cursor.as_deref()).unwrap();
        assert!(next.posts.is_empty() && next.cursor.is_none());
        let th = c.thread_with_replies(p).unwrap().0;
        assert_eq!(th.iter().map(|p| p.text_markdown.as_str()).collect::<Vec<_>>(), ["Grüße 😀 https://e.com/x", "part two"]);
        // SOC-039: one post by its address; a post that is gone is an error
        assert_eq!(c.fetch_post(&p.id).unwrap().text_markdown, "Grüße 😀 https://e.com/x");
        assert!(c.fetch_post("at://x/app.bsky.feed.post/none").unwrap_err().contains("gone"));
        let (bytes, _) = c.download_media(&th[0], &th[0].media[0]).unwrap();
        assert_eq!(bytes, b"JPEGDATA");
        let (bytes, _) = c.download_media(&th[1], &th[1].media[0]).unwrap();
        assert_eq!(bytes, b"VIDEODATA", "the video file comes from the author's server, not the playlist");
        let seen = handle.join().unwrap();
        assert_eq!(seen.len(), 13, "{seen:?}");
    }

    fn page_of(ids: &[&str], cursor: Option<&str>) -> Page {
        Page { posts: ids.iter().map(|i| crate::social::tests::post(i, "bob", 1, None)).collect(), cursor: cursor.map(String::from), top: None }
    }

    fn keys(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|i| crate::social::state::post_key(&crate::social::tests::post(i, "bob", 1, None))).collect()
    }

    // spec: SOC-026
    #[test]
    fn newer_posts_are_the_pages_from_the_top_up_to_the_first_known_post() {
        let pages = vec![page_of(&["n3", "n2"], Some("c1")), page_of(&["n1", "k1", "k0"], Some("c2"))];
        let mut asked: Vec<Option<String>> = vec![];
        let mut it = pages.into_iter();
        let f = newer_with(&keys(&["k1", "k0"]), 200, |c| {
            asked.push(c.map(String::from));
            Ok(it.next().unwrap())
        })
        .unwrap();
        assert_eq!(asked, [None, Some("c1".to_string())]);
        assert_eq!(f.posts.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["n3", "n2", "n1"], "stopped at the first known post");
        assert_eq!(f.gap, None);
        // the first post is known: nothing is new, and no second page is asked for
        let mut n = 0;
        let f = newer_with(&keys(&["k1"]), 200, |_| {
            n += 1;
            Ok(page_of(&["k1", "n9"], Some("c")))
        })
        .unwrap();
        assert_eq!((f.posts.len(), n), (0, 1));
        // the end of the feed also ends the check
        let f = newer_with(&keys(&[]), 200, |_| Ok(page_of(&["a", "b"], None))).unwrap();
        assert_eq!((f.posts.len(), f.gap), (2, None));
    }

    // spec: SOC-026
    #[test]
    fn the_cap_leaves_a_gap_with_the_cursor_to_go_on() {
        let mut n = 0;
        let f = newer_with(&keys(&["old"]), 4, |_| {
            n += 1;
            let ids = [format!("p{n}a"), format!("p{n}b"), format!("p{n}c")];
            Ok(page_of(&ids.iter().map(|s| s.as_str()).collect::<Vec<_>>(), Some(&format!("cur{n}"))))
        })
        .unwrap();
        assert_eq!((n, f.posts.len(), f.gap.as_deref()), (2, 6, Some("cur2")));
        // a post twice in the pages counts once
        let f = newer_with(&keys(&[]), 200, |c| Ok(if c.is_none() { page_of(&["a", "b"], Some("x")) } else { page_of(&["b", "c"], None) })).unwrap();
        assert_eq!(f.posts.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert!(newer_with(&keys(&[]), 200, |_| Err("offline".into())).is_err());
    }

    fn thread_node(n: &str, who: (&str, &str), min: u32, parent: Option<&str>, kids: Vec<Value>, replies: u32) -> Value {
        let mut v = post_view(n, who.0, who.1, &format!("2024-05-17T10:0{min}:00Z"), parent, &format!("post {n}"));
        v["replyCount"] = json!(replies);
        json!({"$type": "app.bsky.feed.defs#threadViewPost", "post": v, "replies": kids})
    }

    /// The answer for the post `p2` of ada: two posts above it, replies of others and of ada herself below it.
    fn conversation_fixture(root_replies: u32) -> Value {
        let (d, cy) = ("did:plc:ada", ("did:plc:cy", "cy.test"));
        let ada = (d, "ada.test");
        let at = |who: &str, n: &str| format!("at://{who}/app.bsky.feed.post/{n}");
        let mut root = thread_node(
            "p2",
            ada,
            2,
            Some("p1"),
            vec![
                thread_node("c1", cy, 3, Some(&at(d, "p2")), vec![thread_node("a9", ada, 9, Some(&at(cy.0, "c1")), vec![], 0)], 1),
                thread_node("p3", ada, 4, Some("p2"), vec![thread_node("p4", ada, 5, Some("p3"), vec![], 0), thread_node("c2", cy, 6, Some(&at(d, "p3")), vec![], 0)], 2),
                json!({"$type": "app.bsky.feed.defs#notFoundPost", "uri": "at://x/y/z", "notFound": true}),
            ],
            root_replies,
        );
        root["parent"] = {
            let mut p1 = thread_node("p1", ada, 1, Some("b0"), vec![], 1);
            p1["parent"] = thread_node("b0", ("did:plc:bob", "bob.test"), 0, None, vec![], 1);
            p1
        };
        json!({"thread": root})
    }

    // spec: SOC-045, SOC-046
    #[test]
    fn the_conversation_of_a_post_from_a_thread_fixture() {
        let resp = conversation_fixture(2);
        let root = parse_post_view(&resp["thread"]["post"]).unwrap();
        let v = thread_view_from_response(&root, &resp);
        let texts = |l: &[Post]| l.iter().map(|p| p.text_markdown.clone()).collect::<Vec<_>>();
        assert_eq!(texts(&v.ancestors), ["post b0", "post p1"], "from the top of the conversation down");
        assert_eq!(v.post.text_markdown, "post p2");
        // depth first, oldest first among siblings; the author's own posts are replies here; a post that is gone is left out
        assert_eq!(v.replies.iter().map(|r| (r.post.text_markdown.as_str(), r.depth)).collect::<Vec<_>>(), [("post c1", 0), ("post a9", 1), ("post p3", 0), ("post p4", 1), ("post c2", 1)]);
        assert!(!v.cut);
        // the post counts more replies than the answer holds (the server stopped at its depth limit)
        let resp = conversation_fixture(3);
        assert!(thread_view_from_response(&root, &resp).cut);
    }

    // spec: SOC-045, SOC-046
    #[test]
    fn the_conversation_of_a_post_is_asked_with_get_post_thread() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let (base, handle) = mock_http(3, move |method, path, headers, _| {
            let host = headers.lines().find_map(|l| l.strip_prefix("Host: ")).unwrap_or("").trim().to_string();
            let did = format!("did:web:{}", host.replace(':', "%3A"));
            match (method, path.split('?').next().unwrap()) {
                ("POST", "/xrpc/com.atproto.server.createSession") => {
                    (200, json!({"accessJwt": "jwt-1", "refreshJwt": "ref-1", "did": did, "handle": "ada.test", "didDoc": {"service": [{"id": "#atproto_pds", "serviceEndpoint": format!("http://{host}")}]}}).to_string())
                }
                ("GET", "/xrpc/app.bsky.feed.getPostThread") if path.contains("missing") => (400, json!({"error": "NotFound", "message": "Post not found"}).to_string()),
                ("GET", "/xrpc/app.bsky.feed.getPostThread") => {
                    assert!(path.contains("depth=10") && path.contains("parentHeight=80"), "{path}");
                    let mut up = thread_node("p1", (&did, "ada.test"), 1, None, vec![], 1);
                    let mut root = thread_node("p2", (&did, "ada.test"), 2, Some("p1"), vec![thread_node("p3", ("did:plc:cy", "cy.test"), 3, Some(&format!("at://{did}/app.bsky.feed.post/p2")), vec![], 0)], 1);
                    up["replies"] = json!([]);
                    root["parent"] = up;
                    (200, json!({"thread": root}).to_string())
                }
                _ => (404, json!({"error": "NotFound"}).to_string()),
            }
        });
        let mut c = Client::new(Account::new(Network::Bluesky, &base, "ada.test", "Ada"), "app-pw");
        let post = crate::social::tests::post("at://x/app.bsky.feed.post/p2", "ada", 1, None);
        let v = c.thread_view(&post).unwrap();
        assert_eq!((v.ancestors.len(), v.post.text_markdown.as_str(), v.replies.len(), v.cut), (1, "post p2", 1, false));
        assert!(c.thread_view(&Post { id: "at://x/app.bsky.feed.post/missing".into(), ..post }).unwrap_err().to_lowercase().contains("not found"));
        assert_eq!(handle.join().unwrap().len(), 3);
    }
}
