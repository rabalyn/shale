//! Social feeds: read the home timeline (and lists / custom feeds) of a Mastodon or Bluesky account
//! and copy posts, whole self-threads and their pictures and videos into the vault as notes.
//!
//! - `mod.rs` (this file): the data model (`Account`, `Post`, `Feed`, `Page` …), the `Client` that
//!   hides the difference between the two networks, and the helpers both share (HTTP, time,
//!   the "self-thread" rule).
//! - `mastodon`, `bluesky`: the two protocols. `markup`: Mastodon's HTML to Markdown.
//! - `note`: `fetch_saved` (thread + media downloads) and `post_note` (the Markdown of the note).
//!
//! All functions that use the network are BLOCKING (they run the system `curl` program, see
//! `dav::request_bytes`): call them from a background thread. Secrets (Mastodon access token,
//! Bluesky app password) are never part of `Account`, never printed in `Debug` output, never put
//! on a command line or in an address; the keyring functions are in `secrets.rs`
//! (`social_lookup` / `social_store` / `social_clear`). The login of a Bluesky account lives in
//! memory only. Media addresses on other servers (picture CDNs) are fetched without any login.

pub mod accounts_ui;
pub mod bluesky;
#[cfg(test)]
mod dataflow_tests;
pub mod demo;
#[cfg(test)]
pub(crate) mod fake_server;
pub mod feed_ui;
pub mod images;
pub mod markup;
pub mod mastodon;
pub mod note;
pub mod post_index;
#[cfg(test)]
mod post_index_tests;
pub mod state;
#[cfg(test)]
mod ui_tests;
pub mod viewer;

use crate::calendar::dav::{self, check_url, Origin};
use crate::calendar::time as t;
use crate::secret::Secret;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use zeroize::{Zeroize, Zeroizing};

// ---------------------------------------------------------------- data model

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Mastodon,
    Bluesky,
}

impl Network {
    /// "Mastodon" / "Bluesky", for menus.
    pub fn label(self) -> &'static str {
        match self {
            Network::Mastodon => "Mastodon",
            Network::Bluesky => "Bluesky",
        }
    }
    /// "mastodon" / "bluesky", as written into notes.
    pub fn key(self) -> &'static str {
        match self {
            Network::Mastodon => "mastodon",
            Network::Bluesky => "bluesky",
        }
    }
}

/// A signed-in account. The secret (access token / app password) is kept elsewhere
/// (`secrets::social_*`), so this can be saved in the settings file as it is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Account {
    /// any unique text, e.g. "mastodon:ada@mastodon.social"
    pub id: String,
    pub network: Network,
    /// https address of the server: the Mastodon instance, or the Bluesky PDS / entryway
    pub server: String,
    /// Mastodon: "ada@mastodon.social"; Bluesky: "ada.bsky.social" (what you sign in with)
    pub handle: String,
    pub display_name: String,
}

/// An account as the settings file holds it: the account and, only when no system keyring took
/// the secret, the secret itself (the settings file can only be read by its owner).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct SocialAccountEntry {
    #[serde(flatten)]
    pub account: Account,
    /// the access token / app password; empty while the keyring holds it
    #[serde(default, skip_serializing_if = "Secret::is_empty")]
    pub secret: Secret,
}

impl std::fmt::Debug for SocialAccountEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SocialAccountEntry").field("account", &self.account).field("secret", &dav::redacted(self.secret.expose())).finish()
    }
}

impl SocialAccountEntry {
    /// Keep the secret of a new account in the keyring (blocking: the keyring may ask for its
    /// password), or in the entry when there is no keyring.
    pub fn new(account: Account, secret: &Secret) -> SocialAccountEntry {
        let secret = crate::secrets::social_store_or_keep(&account.server, &account.handle, secret);
        SocialAccountEntry { account, secret }
    }

    /// The secret, from the keyring or the entry (blocking, like `new`).
    pub fn secret(&self) -> Secret {
        crate::secrets::social_secret(&self.account.server, &self.account.handle, &self.secret)
    }
}

/// The default Bluesky server.
pub const BLUESKY_SERVER: &str = "https://bsky.social";

impl Account {
    /// A new account; the id is made from network and handle.
    pub fn new(network: Network, server: &str, handle: &str, display_name: &str) -> Account {
        let handle = handle.trim().trim_start_matches('@').to_string();
        Account { id: format!("{}:{handle}", network.key()), network, server: server.trim().trim_end_matches('/').to_string(), handle, display_name: display_name.to_string() }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Author {
    /// Mastodon account id / Bluesky DID: identifies the author (handles can change)
    pub id: String,
    /// Mastodon "ada@mastodon.social" (always with the server) / Bluesky "ada.bsky.social"; no leading "@"
    pub handle: String,
    pub display_name: String,
    pub profile_url: String,
    pub avatar_url: String,
}

impl Author {
    /// Display name, or the handle when there is none.
    pub fn name(&self) -> &str {
        if self.display_name.trim().is_empty() {
            &self.handle
        } else {
            &self.display_name
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaKind {
    Image,
    Video,
    /// Mastodon's looping "GIF" videos
    Gifv,
    Audio,
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Media {
    pub kind: MediaKind,
    /// full size picture / the video file (Bluesky: the HLS playlist, see `blob_cid`)
    pub url: String,
    /// small picture for the feed ("" when there is none)
    pub preview_url: String,
    /// description written by the author ("" when none)
    pub alt: String,
    /// a name for the file with an extension, e.g. "photo.jpg"
    pub file_name_hint: String,
    /// Bluesky video: the blob of the real video file (download it from the author's server)
    pub blob_cid: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinkCard {
    pub url: String,
    pub title: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Post {
    pub network: Network,
    /// Mastodon status id / Bluesky `at://` address
    pub id: String,
    /// Bluesky content id ("" on Mastodon)
    pub cid: String,
    /// the post on the web (Mastodon: its `url`; Bluesky: https://bsky.app/profile/<handle>/post/<id>)
    pub url: String,
    pub author: Author,
    /// seconds since 1970 (UTC), 0 when unknown
    pub created_at: i64,
    /// the text as Markdown (links, mentions and hashtags as Markdown links)
    pub text_markdown: String,
    /// Mastodon "spoiler text"; None when there is none
    pub content_warning: Option<String>,
    pub media: Vec<Media>,
    pub quote: Option<Box<Post>>,
    pub link_card: Option<LinkCard>,
    /// the id of the post this is a reply to
    pub reply_to: Option<String>,
    /// set when the timeline shows this post because someone boosted / reposted it
    pub reposted_by: Option<Author>,
    pub reply_count: u32,
    pub boost_count: u32,
    pub like_count: u32,
}

/// One timeline: Mastodon "Home" or a list; Bluesky "Following" or a custom feed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feed {
    /// "home" / "list:<id>" (Mastodon); "timeline" / the feed's `at://` address (Bluesky)
    pub id: String,
    pub title: String,
}

/// The feed a Feeds tab shows: an account (its id) and one of its feeds ("" = the feed used last
/// for the account, or the first; the tab fills it in once the feeds are known).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FeedRef {
    pub account: String,
    pub feed: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    pub posts: Vec<Post>,
    /// pass it to `timeline` for the next (older) page; None at the end
    pub cursor: Option<String>,
    /// Mastodon: the id of the newest timeline entry of the page (a boost has its own id, not the
    /// one of the post); what the next check for newer posts asks after. None on Bluesky.
    pub top: Option<String>,
}

/// The most posts one check for newer posts brings in (SOC-026).
pub const MAX_NEW: usize = 200;

/// What a check for newer posts is told about the posts already there.
#[derive(Clone, Debug, Default)]
pub struct Since {
    /// Mastodon: the id of the newest timeline entry known (`Page::top`)
    pub top: Option<String>,
    /// Bluesky: the keys (`state::post_key`) of the posts known, in the list or waiting
    pub known: HashSet<String>,
}

/// What a check for newer posts found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fresh {
    /// the new posts, newest first
    pub posts: Vec<Post>,
    /// Mastodon: the id of the newest timeline entry now known
    pub top: Option<String>,
    /// Bluesky: the check stopped at its limit before it met a known post; this cursor continues
    /// below the last of `posts`
    pub gap: Option<String>,
}

// ---------------------------------------------------------------- HTTP

/// An error answer of a server.
#[derive(Clone, Debug, PartialEq)]
pub struct ApiError {
    pub status: u16,
    /// Bluesky error name (`ExpiredToken`) or Mastodon's `error` text; may be empty
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<ApiError> for String {
    fn from(e: ApiError) -> String {
        e.message
    }
}

impl ApiError {
    fn plain(message: impl Into<String>) -> ApiError {
        ApiError { status: 0, code: String::new(), message: message.into() }
    }
}

/// Biggest JSON answer; the biggest media file Shale downloads.
const MAX_JSON: u64 = 20_000_000;
pub const MAX_MEDIA_BYTES: u64 = 64_000_000;

fn no_login() -> dav::Account {
    dav::Account { url: String::new(), username: String::new(), password: Secret::default() }
}

/// Overwrite every string in a JSON answer or request that held a secret (the tree would
/// otherwise leave the text in freed memory).
pub(crate) fn wipe_value(v: &mut Value) {
    match v {
        Value::String(s) => s.zeroize(),
        Value::Array(a) => a.iter_mut().for_each(wipe_value),
        Value::Object(o) => o.iter_mut().for_each(|(_, x)| wipe_value(x)),
        _ => {}
    }
}

/// A secret text out of a JSON value (empty when it is not a string).
pub(crate) fn secret_of(v: &Value) -> Secret {
    Secret::new(v.as_str().unwrap_or(""))
}

/// The JSON as text in a buffer of exactly the right size (found by a first, counting pass), so
/// nothing is left behind by growing; wiped when dropped.
fn json_text(v: &Value) -> Zeroizing<String> {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0 += b.len();
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut n = Count(0);
    let _ = serde_json::to_writer(&mut n, v);
    let mut out = Zeroizing::new(Vec::with_capacity(n.0));
    let _ = serde_json::to_writer(&mut *out, v);
    // what serde_json wrote is UTF-8; the empty string is the (unreachable) failure
    Zeroizing::new(String::from_utf8(std::mem::take(&mut *out)).unwrap_or_default())
}

/// One JSON request to an API. `token`: sent as `Authorization: Bearer …` (inside curl's input,
/// never on its command line); only ever to this one address.
pub(crate) fn api(method: &str, url: &str, token: Option<&str>, body: Option<&Value>) -> Result<Value, ApiError> {
    let mut hs = vec!["Accept: application/json"];
    // the body can hold a password or client secret, the header holds the token: both are wiped
    let text = body.map(json_text).unwrap_or_default();
    if body.is_some() {
        hs.push("Content-Type: application/json");
    }
    let auth;
    if let Some(tok) = token {
        if tok.chars().any(char::is_control) {
            return Err(ApiError::plain("the access token contains line breaks or control characters"));
        }
        auth = crate::secret::concat(&["Authorization: Bearer ", tok]);
        hs.push(&auth);
    }
    let r = dav::request_bytes(method, url, &hs, &text, &no_login(), MAX_JSON).map_err(ApiError::plain)?;
    let (status, answer) = (r.status, Zeroizing::new(r.body));
    let json: Value = serde_json::from_slice(&answer).unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return if json.is_null() && !answer.iter().all(|b| b.is_ascii_whitespace()) { Err(ApiError::plain("the server's answer could not be read")) } else { Ok(json) };
    }
    // Bluesky: {"error": "ExpiredToken", "message": "…"}; Mastodon: {"error": "…"}
    let code = json["error"].as_str().unwrap_or("").to_string();
    let detail = json["message"].as_str().or_else(|| json["error_description"].as_str()).or_else(|| json["error"].as_str()).unwrap_or("");
    let message = match status {
        401 | 403 => format!("the server refused the login{}", if detail.is_empty() { String::new() } else { format!(" ({detail})") }),
        404 => format!("not found (404){}", if detail.is_empty() { String::new() } else { format!(": {detail}") }),
        429 => "the server says: too many requests, try again in a few minutes".to_string(),
        s => format!("the server answered with status {s}{}", if detail.is_empty() { String::new() } else { format!(": {detail}") }),
    };
    Err(ApiError { status, code, message })
}

/// Download a media file (no login is sent). At most `MAX_MEDIA_BYTES`. Returns the bytes and
/// the content type, if the server named one.
pub fn download(url: &str) -> Result<(Vec<u8>, Option<String>), String> {
    check_url(url)?;
    let r = dav::request_bytes("GET", url, &[], "", &no_login(), MAX_MEDIA_BYTES)?;
    if !(200..300).contains(&r.status) {
        return Err(format!("the server answered with status {}", r.status));
    }
    if r.body.is_empty() {
        return Err("the file is empty".into());
    }
    Ok((r.body, r.content_type))
}

/// A server address: https, no path, no trailing slash ("mastodon.social" works too).
pub fn clean_server(s: &str) -> Result<String, String> {
    let s = s.trim().trim_end_matches('/');
    let s = if s.contains("://") { s.to_string() } else { format!("https://{s}") };
    let o: Origin = check_url(&s)?;
    let default = if o.scheme == "https" { 443 } else { 80 };
    Ok(format!("{}://{}{}", o.scheme, o.host, if o.port == default { String::new() } else { format!(":{}", o.port) }))
}

/// Percent-encode for a query parameter or form value.
pub(crate) fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `?a=1&b=2` (empty without parameters).
pub(crate) fn query(params: &[(&str, &str)]) -> String {
    if params.is_empty() {
        return String::new();
    }
    format!("?{}", params.iter().map(|(k, v)| format!("{k}={}", url_encode(v))).collect::<Vec<_>>().join("&"))
}

// ---------------------------------------------------------------- small helpers

/// "2024-05-17T10:30:00.123Z" / "…+02:00" (RFC 3339, also with a space instead of the T) → seconds since 1970.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() < 19 || !s.is_char_boundary(19) {
        return None;
    }
    let num = |a: usize, z: usize| s.get(a..z)?.parse::<u32>().ok();
    if b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (y, mo, d, h, mi, sec) = (s.get(0..4)?.parse::<i32>().ok()?, num(5, 7)?, num(8, 10)?, num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&mo) || d < 1 || d > t::days_in_month(y, mo) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let mut rest = &s[19..];
    if let Some(r) = rest.strip_prefix('.') {
        let digits = r.chars().take_while(|c| c.is_ascii_digit()).count();
        rest = &r[digits..];
    }
    let offset = match rest {
        "" | "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let r = &rest[1..];
            let (oh, om) = (r.get(0..2)?.parse::<i64>().ok()?, r.get(r.len().checked_sub(2)?..)?.parse::<i64>().ok()?);
            sign * (oh * 3600 + om * 60)
        }
    };
    Some(t::days_from_civil(y, mo, d) * 86_400 + (h * 3600 + mi * 60 + sec) as i64 - offset)
}

/// A file name (with extension) from a media address: its last path segment; Bluesky's
/// `<cid>@jpeg` becomes `<cid>.jpeg`. `fallback` is used when the address says nothing.
pub(crate) fn file_name_from_url(url: &str, fallback: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let last = path.rsplit('/').next().unwrap_or("");
    let last = last.replace('@', ".").replace(':', "_");
    let last = last.trim_matches('.');
    if last.is_empty() || last.len() > 120 {
        fallback.to_string()
    } else if !last.contains('.') {
        format!("{last}.{}", fallback.rsplit('.').next().unwrap_or("bin"))
    } else {
        last.to_string()
    }
}

pub(crate) fn text(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

pub(crate) fn count(v: &Value) -> u32 {
    v.as_u64().unwrap_or(0).min(u32::MAX as u64) as u32
}

// ---------------------------------------------------------------- self-thread

/// The "thread of one author" around `root`:
/// - the ancestors that are by the same author, as far up as the run goes without another author;
/// - then `root`;
/// - then the descendants by the same author that reply to a post already in the chain, in the
///   order they were written.
///
/// `ancestors` run from the top of the conversation down to the parent of `root`; `descendants`
/// are all replies below `root`, in any order. The result is in reading order and includes `root`.
pub fn self_thread(root: Post, ancestors: Vec<Post>, descendants: Vec<Post>) -> Vec<Post> {
    let author = root.author.id.clone();
    let mut before: Vec<Post> = ancestors.into_iter().rev().take_while(|p| p.author.id == author).collect();
    before.reverse();
    let mut chain = before;
    let root_at = chain.len();
    chain.push(root);
    let mut ids: std::collections::HashSet<String> = chain.iter().map(|p| p.id.clone()).collect();
    let mut later: Vec<Post> = descendants.into_iter().filter(|p| p.author.id == author).collect();
    later.sort_by_key(|p| p.created_at);
    // repeat until nothing more joins (a reply that was stored before its parent's time, odd clocks)
    loop {
        let before_len = chain.len();
        let mut rest = Vec::new();
        for p in later {
            match &p.reply_to {
                Some(parent) if ids.contains(parent) => {
                    ids.insert(p.id.clone());
                    chain.push(p);
                }
                _ => rest.push(p),
            }
        }
        later = rest;
        if chain.len() == before_len || later.is_empty() {
            break;
        }
    }
    // the posts after the root in the order they were written
    chain[root_at + 1..].sort_by_key(|p| p.created_at);
    chain
}

/// Most replies of other people that are kept with a saved post.
pub const MAX_REPLIES: usize = 300;

/// A reply of somebody else (or a reply of the author to somebody else), shown below the thread.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub post: Post,
    /// 0: answers a post of the thread itself; 1: answers a reply of depth 0 …
    pub depth: usize,
}

/// The conversation below the self-`thread`: `descendants` (all replies below the chosen post, in
/// any order) without the posts of the thread itself, depth first, oldest first among siblings.
/// Replies whose parent is not in the fetched context are left out.
pub fn conversation(thread: &[Post], descendants: &[Post]) -> Vec<Reply> {
    let in_thread: std::collections::HashSet<&str> = thread.iter().map(|p| p.id.as_str()).collect();
    let mut kids: std::collections::HashMap<&str, Vec<&Post>> = std::collections::HashMap::new();
    for p in descendants.iter().filter(|p| !in_thread.contains(p.id.as_str())) {
        if let Some(parent) = &p.reply_to {
            kids.entry(parent.as_str()).or_default().push(p);
        }
    }
    for v in kids.values_mut() {
        v.sort_by_key(|p| p.created_at);
    }
    fn walk(id: &str, depth: usize, kids: &std::collections::HashMap<&str, Vec<&Post>>, seen: &mut std::collections::HashSet<String>, out: &mut Vec<Reply>) {
        for p in kids.get(id).into_iter().flatten() {
            if out.len() >= MAX_REPLIES || !seen.insert(p.id.clone()) {
                continue;
            }
            out.push(Reply { post: Post { reposted_by: None, ..(*p).clone() }, depth });
            walk(&p.id, depth + 1, kids, seen, out);
        }
    }
    let mut out = vec![];
    let mut seen = std::collections::HashSet::new();
    for t in thread {
        walk(&t.id, 0, &kids, &mut seen, &mut out);
    }
    out
}

// ---------------------------------------------------------------- the client

/// A signed-in account ready to use: one type for both networks. It is `Clone + Send`: give a
/// clone to each background job (a clone made before a Bluesky login expired logs in again by
/// itself). Debug output never shows the secret.
#[derive(Clone)]
pub struct Client {
    pub account: Account,
    backend: Backend,
}

#[derive(Clone)]
enum Backend {
    Mastodon { token: Secret },
    Bluesky(bluesky::Api),
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("account", &self.account).field("secret", &dav::redacted("x")).finish()
    }
}

impl Client {
    /// `secret`: the Mastodon access token or the Bluesky app password. Nothing is sent yet.
    pub fn new(account: Account, secret: &str) -> Client {
        let backend = match account.network {
            Network::Mastodon => Backend::Mastodon { token: Secret::new(secret.trim()) },
            Network::Bluesky => Backend::Bluesky(bluesky::Api::new(&account.server, &account.handle, secret)),
        };
        Client { account, backend }
    }

    /// The timelines to choose from.
    pub fn feeds(&mut self) -> Result<Vec<Feed>, String> {
        match &mut self.backend {
            Backend::Mastodon { token } => mastodon::feeds(&self.account.server, token.expose()),
            Backend::Bluesky(api) => bluesky::feeds(api),
        }
    }

    /// One page of a timeline, newest first. `cursor`: None for the first page, then the
    /// `Page::cursor` of the page before.
    pub fn timeline(&mut self, feed: &Feed, cursor: Option<&str>) -> Result<Page, String> {
        match &mut self.backend {
            Backend::Mastodon { token } => mastodon::timeline(&self.account.server, token.expose(), feed, cursor),
            Backend::Bluesky(api) => bluesky::timeline(api, feed, cursor),
        }
    }

    /// The posts newer than the known ones, newest first, at most `MAX_NEW` (see `Fresh`).
    pub fn newer(&mut self, feed: &Feed, since: &Since) -> Result<Fresh, String> {
        match &mut self.backend {
            Backend::Mastodon { token } => mastodon::newer(&self.account.server, token.expose(), feed, since.top.as_deref(), MAX_NEW),
            Backend::Bluesky(api) => bluesky::newer(api, feed, &since.known, MAX_NEW),
        }
    }

    /// One post by its id (to open a search hit the index and the feed no longer hold, SOC-039).
    pub fn fetch_post(&mut self, id: &str) -> Result<Post, String> {
        match &mut self.backend {
            Backend::Mastodon { token } => mastodon::fetch_post(&self.account.server, token.expose(), id),
            Backend::Bluesky(api) => bluesky::fetch_post(api, id),
        }
    }

    /// The whole self-thread of a post (see `self_thread`), in reading order, including the post, and the
    /// replies of other people below it (see `conversation`).
    pub fn thread_with_replies(&mut self, post: &Post) -> Result<(Vec<Post>, Vec<Reply>), String> {
        match &mut self.backend {
            Backend::Mastodon { token } => mastodon::thread_full(&self.account.server, token.expose(), post),
            Backend::Bluesky(api) => bluesky::thread_full(api, post),
        }
    }

    /// The bytes of one media file of `post` (the real video file on Bluesky). No login is sent to media servers.
    pub fn download_media(&self, post: &Post, media: &Media) -> Result<(Vec<u8>, Option<String>), String> {
        match (&self.backend, &media.blob_cid) {
            (Backend::Bluesky(_), Some(cid)) => bluesky::download_blob(&post.author.id, cid),
            _ => download(&media.url),
        }
    }

    /// Fetch the thread of `post` and all its media in one go, ready for `note::post_note`.
    pub fn save(&mut self, post: &Post) -> note::SavedPost {
        note::fetch_saved(self, post)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // spec: SEC-017
    #[test]
    fn json_with_secrets_is_written_exactly_and_wiped_after_use() {
        let mut v = serde_json::json!({"identifier": "ada", "password": "hunter2", "list": ["x", {"deep": "y"}], "n": 5, "ü": "ö"});
        assert_eq!(*json_text(&v), v.to_string());
        wipe_value(&mut v);
        assert_eq!(v["password"], "", "strings are emptied");
        assert_eq!(v["list"][1]["deep"], "");
        assert_eq!(v["n"], 5, "numbers stay");
        assert_eq!(secret_of(&serde_json::json!("tok")).expose(), "tok");
        assert!(secret_of(&serde_json::json!(5)).is_empty());
    }

    pub fn post(id: &str, author: &str, at: i64, reply_to: Option<&str>) -> Post {
        Post {
            network: Network::Mastodon,
            id: id.into(),
            cid: String::new(),
            url: format!("https://m.test/@{author}/{id}"),
            author: Author { id: author.into(), handle: format!("{author}@m.test"), display_name: author.to_uppercase(), profile_url: format!("https://m.test/@{author}"), avatar_url: String::new() },
            created_at: at,
            text_markdown: format!("text of {id}"),
            content_warning: None,
            media: vec![],
            quote: None,
            link_card: None,
            reply_to: reply_to.map(String::from),
            reposted_by: None,
            reply_count: 0,
            boost_count: 0,
            like_count: 0,
        }
    }

    #[test]
    fn rfc3339_times() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2024-02-29T12:30:45.123Z"), Some(1_709_209_845));
        assert_eq!(parse_rfc3339("2024-02-29T14:30:45+02:00"), Some(1_709_209_845));
        assert_eq!(parse_rfc3339("2024-02-29T07:00:45-05:30"), Some(1_709_209_845));
        assert_eq!(parse_rfc3339("2024-02-29 12:30:45.5+00:00"), Some(1_709_209_845));
        assert_eq!(parse_rfc3339("2023-02-29T12:30:45Z"), None);
        assert_eq!(parse_rfc3339("2024-13-01T00:00:00Z"), None);
        assert_eq!(parse_rfc3339("yesterday"), None);
        assert_eq!(parse_rfc3339("2024-02-29T12:30:45+2"), None);
        assert_eq!(parse_rfc3339("2024-02-29Tä2:30:45Z"), None);
    }

    #[test]
    fn urls_and_names() {
        assert_eq!(url_encode("a b&c/ü"), "a%20b%26c%2F%C3%BC");
        assert_eq!(query(&[("limit", "20"), ("max_id", "1 2")]), "?limit=20&max_id=1%202");
        assert_eq!(clean_server("Mastodon.Social/").unwrap(), "https://mastodon.social");
        assert_eq!(clean_server("https://bsky.social:443").unwrap(), "https://bsky.social");
        assert_eq!(clean_server("http://localhost:8080").unwrap(), "http://localhost:8080");
        assert!(clean_server("http://mastodon.social").is_err());
        assert!(clean_server("https://user:pw@mastodon.social").is_err());
        assert_eq!(file_name_from_url("https://cdn.bsky.app/img/feed_fullsize/plain/did:plc:abc/bafkrei123@jpeg", "x.jpg"), "bafkrei123.jpeg");
        assert_eq!(file_name_from_url("https://files.m.test/media/original/abc.png?x=1", "x.jpg"), "abc.png");
        assert_eq!(file_name_from_url("https://files.m.test/media/abc", "x.jpg"), "abc.jpg");
        assert_eq!(file_name_from_url("https://files.m.test/", "x.jpg"), "x.jpg");
    }

    #[test]
    fn network_names_and_debug_redaction() {
        assert_eq!(Network::Bluesky.key(), "bluesky");
        let acct = Account::new(Network::Mastodon, "https://m.test/", "@ada@m.test", "Ada");
        assert_eq!((acct.id.as_str(), acct.server.as_str(), acct.handle.as_str()), ("mastodon:ada@m.test", "https://m.test", "ada@m.test"));
        let c = Client::new(acct, "tok-secret-1");
        assert!(!format!("{c:?}").contains("tok-secret-1"));
        let b = Client::new(Account::new(Network::Bluesky, BLUESKY_SERVER, "ada.bsky.social", ""), "pw-secret-2");
        assert!(!format!("{b:?}").contains("pw-secret-2"));
        let json = serde_json::to_string(&c.account).unwrap();
        assert!(json.contains("mastodon") && !json.contains("tok-secret"));
    }

    // spec: SOC-019
    #[test]
    fn replies_of_others_are_a_tree_without_the_authors_own_thread() {
        let p = |id: &str, a: &str, t: i64, r: &str| post(id, a, t, Some(r));
        let thread = vec![post("a1", "ada", 1, None), p("a2", "ada", 2, "a1")];
        let desc = vec![p("c2", "cy", 6, "c1"), p("c1", "cy", 3, "a2"), p("d1", "dee", 4, "a1"), p("a2", "ada", 2, "a1"), p("e1", "eve", 7, "c2"), p("x", "zed", 8, "missing"), p("a9", "ada", 9, "c1")];
        let r = conversation(&thread, &desc);
        let shape: Vec<(&str, usize)> = r.iter().map(|r| (r.post.id.as_str(), r.depth)).collect();
        // under a1: d1; under a2: c1 → (a9 at time 9 after c2 at 6) c2 → e1; the orphan x is dropped
        assert_eq!(shape, [("d1", 0), ("c1", 0), ("c2", 1), ("e1", 2), ("a9", 1)]);
        assert!(conversation(&thread, &[]).is_empty());
    }

    #[test]
    fn self_thread_rule() {
        let p = |id: &str, a: &str, t: i64, r: Option<&str>| post(id, a, t, r);
        // bob starts, ada answers (root clicked = a2): ada's chain is a2 + her later replies
        let anc = vec![p("b1", "bob", 1, None), p("a1", "ada", 2, Some("b1"))];
        let root = p("a2", "ada", 3, Some("a1"));
        let desc = vec![p("c1", "cy", 4, Some("a2")), p("a4", "ada", 6, Some("a3")), p("a3", "ada", 5, Some("a2")), p("a9", "ada", 9, Some("c1")), p("b2", "bob", 7, Some("a3"))];
        let ids: Vec<String> = self_thread(root, anc, desc).into_iter().map(|p| p.id).collect();
        // above a2 only a1 is ada's: bob's b1 ends the run
        assert_eq!(ids, ["a1", "a2", "a3", "a4"], "a9 answers somebody else's reply, so it is left out");
        // a different author directly above stops the run
        let anc = vec![p("a1", "ada", 1, None), p("b1", "bob", 2, Some("a1"))];
        let ids: Vec<String> = self_thread(p("a2", "ada", 3, Some("b1")), anc, vec![]).into_iter().map(|p| p.id).collect();
        assert_eq!(ids, ["a2"]);
    }
}
