//! A small CalDAV client. HTTP(S) goes through the system's `curl` program (TLS with the system
//! certificates). The user name, password and request body are passed to curl on its standard
//! input, never on the command line.
//!
//! - `discover`: from a server/account address to the list of calendars
//!   (current-user-principal → calendar-home-set → calendar collections).
//! - `calendar_state`: the calendar's version tag (getctag / sync-token) and whether it is writable.
//! - `fetch_events`: a calendar-query REPORT for a time range.
//! - `put_event` / `delete_event`: write or remove one event (with If-Match / If-None-Match).
//!
//! Security rules, all enforced in `request_with` so no caller can skip them:
//! - only `https://` addresses; `http://` only for servers on this computer (localhost);
//! - no user name or password inside an address;
//! - the user's `~/.curlrc` is ignored (`-q`), redirects only to allowed addresses, and curl
//!   never sends the password along a redirect to another server;
//! - header values with line breaks or control characters are refused (no header injection).
//!
//! On top of that, the addresses a server hands out for events (`href`) are only used when they
//! are on the same server as the calendar, so a server can't make Shale send the password to
//! somewhere else.

use crate::busy::Cancel;
use crate::calendar::ics::{self, Event};
use roxmltree::{Document, Node};
use std::io::Write;
use std::process::Stdio;
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, PartialEq)]
pub struct Account {
    /// for calendars: the calendar's address (the server addresses of events are checked against it)
    pub url: String,
    pub username: String,
    pub password: crate::secret::Secret,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account").field("url", &self.url).field("username", &self.username).field("password", &redacted(self.password.expose())).finish()
    }
}

/// How a password shows up in debug output.
pub fn redacted(password: &str) -> &'static str {
    if password.is_empty() {
        ""
    } else {
        "***"
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FoundCalendar {
    pub url: String,
    pub name: String,
    pub color: Option<String>,
    /// the server says this account may not change it
    pub read_only: bool,
}

// ---------------------------------------------------------------- addresses

/// Scheme, host (lower case, IPv6 in brackets) and port of an http(s) address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

impl Origin {
    pub fn parse(url: &str) -> Result<Origin, String> {
        let url = url.trim();
        let (scheme, rest) = url.split_once("://").ok_or_else(|| format!("not a web address: {url}"))?;
        let scheme = scheme.to_ascii_lowercase();
        let default_port = match scheme.as_str() {
            "https" => 443,
            "http" => 80,
            _ => return Err(format!("only https:// addresses are supported, not {scheme}://")),
        };
        let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
        if authority.contains('@') {
            return Err("the address must not contain a user name or password; enter them in their own fields".into());
        }
        let (host, port) = if let Some(after) = authority.strip_prefix('[') {
            let (h, tail) = after.split_once(']').ok_or("broken IPv6 address")?;
            if h.is_empty() || !h.chars().all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.') {
                return Err(format!("bad server name in {url}"));
            }
            (format!("[{}]", h.to_ascii_lowercase()), tail)
        } else {
            let (h, tail) = match authority.rfind(':') {
                Some(p) => (&authority[..p], &authority[p..]),
                None => (authority, ""),
            };
            let h = h.trim_end_matches('.').to_lowercase();
            if h.is_empty() || !h.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '.' || c == '_') {
                return Err(format!("bad server name in {url}"));
            }
            (h, tail)
        };
        let port = if port.is_empty() || port == ":" { default_port } else { port.strip_prefix(':').and_then(|p| p.parse().ok()).ok_or_else(|| format!("bad port in {url}"))? };
        Ok(Origin { scheme, host, port })
    }

    /// A server on this computer.
    pub fn is_local(&self) -> bool {
        let h = self.host.as_str();
        h == "localhost" || h.ends_with(".localhost") || h == "[::1]" || h.parse::<std::net::Ipv4Addr>().map(|a| a.is_loopback()).unwrap_or(false)
    }
}

/// An address Shale may send credentials to: https, or http for this computer only.
pub fn check_url(url: &str) -> Result<Origin, String> {
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("the address contains spaces or line breaks".into());
    }
    let o = Origin::parse(url)?;
    if o.scheme == "http" && !o.is_local() {
        return Err("only https:// addresses are allowed: http:// would send your password unencrypted (it is only accepted for a server on this computer)".into());
    }
    Ok(o)
}

/// Both addresses are on the same server (scheme, host and port).
pub fn same_server(a: &str, b: &str) -> bool {
    matches!((Origin::parse(a), Origin::parse(b)), (Ok(x), Ok(y)) if x == y)
}

/// The host of an address, for messages.
pub fn host_of(url: &str) -> String {
    Origin::parse(url).map(|o| o.host).unwrap_or_else(|_| url.to_string())
}

fn has_scheme(href: &str) -> bool {
    match href.find(':') {
        Some(p) if p > 0 => {
            let s = &href[..p];
            s.starts_with(|c: char| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
        }
        _ => false,
    }
}

/// Make an href from the server absolute.
pub fn resolve(base: &str, href: &str) -> String {
    let href = href.trim();
    if has_scheme(href) {
        return href.to_string();
    }
    let scheme_end = base.find("://").map(|p| p + 3).unwrap_or(0);
    if let Some(rest) = href.strip_prefix("//") {
        return format!("{}{rest}", &base[..scheme_end]);
    }
    let origin_end = base[scheme_end..].find('/').map(|p| scheme_end + p).unwrap_or(base.len());
    if href.starts_with('/') {
        format!("{}{}", &base[..origin_end], href)
    } else {
        let dir = match base.rfind('/') {
            Some(p) if p >= origin_end => &base[..=p],
            _ => &base[..origin_end],
        };
        format!("{}{}{href}", dir, if dir.ends_with('/') { "" } else { "/" })
    }
}

// ---------------------------------------------------------------- HTTP through curl

/// Largest answer accepted from a server.
const MAX_BYTES: u64 = 52_428_800;
const MARK: &str = "\n__SHALE__";
// (%{size_header}: bytes of all response headers, which `-i` puts before the body; works with
// any curl, unlike %header{…} which needs curl 7.84)
const WRITE_OUT: &str = "\n__SHALE__%{http_code}\t%{size_header}\t%{url_effective}";
const OTHER_SERVER: &str = "the event's address on the server points to a different server; Shale does not send your password there";

#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub status: u16,
    pub body: String,
    /// where the answer came from in the end (after redirects)
    pub url: String,
    /// the ETag header of the answer
    pub etag: Option<String>,
}

/// Append `parts` as one curl config string: backslash escapes, and line breaks must be written
/// as \r / \n. `out` must have room (twice the text plus two) or it will grow, and a grown
/// `String` leaves its old buffer unwiped; `request_bytes_opts` sizes it ahead.
fn quote_into(out: &mut String, parts: &[&str]) {
    out.push('"');
    for c in parts.iter().flat_map(|p| p.chars()) {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn has_control(s: &str) -> bool {
    s.chars().any(char::is_control)
}

/// A version tag as the server sent it, or None when it is empty or could be used to inject headers.
pub fn clean_etag(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty() && !has_control(s) && !s.contains("%header")).then(|| s.to_string())
}

/// Tests only: a certificate authority that curl trusts instead of the system's (the test
/// environment's). There is no such switch outside tests.
#[cfg(test)]
pub(crate) static TEST_CA: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

/// Run one HTTP request with curl.
pub fn request(method: &str, url: &str, depth: &str, body: &str, acct: &Account) -> Result<Response, String> {
    request_with(method, url, &[&format!("Depth: {depth}"), "Content-Type: application/xml; charset=utf-8"], body, acct)
}

/// One HTTP request with any headers; the answer's body as text.
pub fn request_with(method: &str, url: &str, headers: &[&str], body: &str, acct: &Account) -> Result<Response, String> {
    request_opts(method, url, headers, body, acct, true)
}

/// Like [`request_with`]; `follow` = false never follows a redirect (the OAuth2 token endpoint:
/// its form holds secrets and must go only where it was addressed).
pub fn request_opts(method: &str, url: &str, headers: &[&str], body: &str, acct: &Account, follow: bool) -> Result<Response, String> {
    let mut r = request_bytes_opts(method, url, headers, body, acct, MAX_BYTES, How { follow, upload: None })?;
    let text = String::from_utf8_lossy(&r.body).into_owned();
    r.body.zeroize();
    Ok(Response { status: r.status, body: text, url: r.url, etag: r.etag })
}

/// The answer to `request_bytes`: the body is raw (pictures, video).
#[derive(Clone, Debug, PartialEq)]
pub struct BytesResponse {
    pub status: u16,
    pub body: Vec<u8>,
    /// where the answer came from in the end (after redirects)
    pub url: String,
    pub etag: Option<String>,
    /// the Content-Type header of the answer (without parameters, lower case)
    pub content_type: Option<String>,
}

/// One HTTP request with any headers and a binary answer of at most `max_bytes` (bigger answers
/// are an error). All the security rules of this file apply; a header `Authorization: …` is only
/// accepted when the answer finally comes from the same server (curl itself drops it on a
/// redirect to another server).
pub fn request_bytes(method: &str, url: &str, headers: &[&str], body: &str, acct: &Account, max_bytes: u64) -> Result<BytesResponse, String> {
    request_bytes_opts(method, url, headers, body, acct, max_bytes, How { follow: true, upload: None })
}

/// A file that curl sends as the body of a request (read by curl itself, so the bytes never pass
/// through a text field).
#[derive(Clone, Copy, Debug)]
pub enum Upload<'a> {
    /// the whole body is the file (`data-binary @file`); the caller sets `Content-Type`
    Raw { path: &'a std::path::Path },
    /// a `multipart/form-data` body: the file as `field`, and these text fields
    Form { field: &'a str, path: &'a std::path::Path, mime: &'a str, fields: &'a [(&'a str, &'a str)] },
}

/// How a request is sent besides its method, address and headers.
#[derive(Clone, Copy)]
struct How<'a> {
    /// follow redirects
    follow: bool,
    upload: Option<Upload<'a>>,
}

/// Whether a file path or type can stand in curl's `form` and `data-binary` values unchanged: no
/// `"`, `;`, `,` (curl's form syntax) and no control character (SEC-026). A backslash, a colon or a
/// tilde are fine: the config's quoting escapes the backslash (`quote_into`), and curl's form syntax
/// reads it as plain outside of quotes, so a Windows temp path goes through.
fn curl_plain(s: &str) -> bool {
    !s.chars().any(|c| c.is_control() || matches!(c, '"' | ';' | ','))
}

/// Like [`request_bytes`] with a file as the body (SOC-053). The path must be one Shale made (no
/// `"`, `;`, `,` or control character in it; a backslash is a normal part of a Windows path). Uploads may take longer than other requests.
pub fn request_upload(method: &str, url: &str, headers: &[&str], upload: Upload, acct: &Account, max_bytes: u64) -> Result<BytesResponse, String> {
    let (path, mime) = match upload {
        Upload::Raw { path } => (path, ""),
        Upload::Form { path, mime, .. } => (path, mime),
    };
    let p = path.to_str().ok_or("the file name is not text")?;
    if !curl_plain(p) || !curl_plain(mime) {
        return Err("the file cannot be sent (its name or type has special characters)".into());
    }
    request_bytes_opts(method, url, headers, "", acct, max_bytes, How { follow: true, upload: Some(upload) })
}

/// The curl command of a request: its arguments hold no user name, password, header or body (they
/// arrive on standard input, `-K -`, SEC-002), and `-q` comes first so that `~/.curlrc` is ignored.
fn curl_command(follow: bool, protos: &str, max_time: &str, max_bytes: u64, method: &str, url: &str) -> std::process::Command {
    let mut curl = crate::program::command("curl");
    // -q must come first: don't read ~/.curlrc (it could turn off certificate checks or trace the password to a file)
    curl.args(["-q", "-sS", "-i"]).args(if follow { &["-L", "--max-redirs", "5"][..] } else { &[][..] }).args(["--proto", protos, "--proto-redir", protos]).args([
        "--connect-timeout",
        "10",
        "--max-time",
        max_time,
        "--max-filesize",
        &max_bytes.to_string(),
    ]);
    // tests only: trust the test environment's certificate authority (tests/caldav-env/, CALENV-002)
    #[cfg(test)]
    if let Some(ca) = TEST_CA.lock().unwrap().as_ref() {
        curl.arg("--cacert").arg(ca);
    }
    curl.args(["-X", method, "-w", WRITE_OUT, "-K", "-", url]);
    curl
}

fn request_bytes_opts(method: &str, url: &str, headers: &[&str], body: &str, acct: &Account, max_bytes: u64, how: How) -> Result<BytesResponse, String> {
    let How { follow, upload } = how;
    let origin = check_url(url)?;
    if !method.chars().all(|c| c.is_ascii_uppercase()) || method.is_empty() {
        return Err(format!("bad request method {method:?}"));
    }
    if let Some(h) = headers.iter().find(|h| has_control(h)) {
        return Err(format!("refusing to send the header “{}”: it contains line breaks or control characters", h.split(':').next().unwrap_or("")));
    }
    if acct.username.contains(':') || has_control(&acct.username) {
        return Err("the user name must not contain ':' or control characters".into());
    }
    if acct.password.expose().contains('\0') || body.contains('\0') {
        return Err("the password or the event contains a NUL character".into());
    }
    // the config holds the password, the body and the headers (tokens): sized ahead so that it never
    // reallocates, and zeroed when dropped (SEC-017)
    let room = 2 * (acct.username.len() + acct.password.len() + body.len() + headers.iter().map(|h| h.len()).sum::<usize>()) + 40 * (headers.len() + 3);
    let mut cfg = Zeroizing::new(String::with_capacity(room));
    if !acct.username.is_empty() {
        cfg.push_str("user = ");
        quote_into(&mut cfg, &[&acct.username, ":", acct.password.expose()]);
        cfg.push('\n');
    }
    if !body.is_empty() {
        cfg.push_str("data-binary = ");
        quote_into(&mut cfg, &[body]);
        cfg.push('\n');
    }
    match upload {
        Some(Upload::Raw { path }) => {
            cfg.push_str("data-binary = ");
            quote_into(&mut cfg, &["@", &path.to_string_lossy()]);
            cfg.push('\n');
        }
        Some(Upload::Form { field, path, mime, fields }) => {
            cfg.push_str("form = ");
            quote_into(&mut cfg, &[field, "=@", &path.to_string_lossy(), ";type=", mime]);
            cfg.push('\n');
            for (k, v) in fields {
                cfg.push_str("form-string = ");
                quote_into(&mut cfg, &[k, "=", v]);
                cfg.push('\n');
            }
        }
        None => {}
    }
    for h in headers {
        cfg.push_str("header = ");
        quote_into(&mut cfg, &[h]);
        cfg.push('\n');
    }
    let with_auth = headers.iter().any(|h| h.trim_start().to_ascii_lowercase().starts_with("authorization:"));
    // a local http server may redirect to http; the final address is checked below
    let protos = if origin.scheme == "http" { "=http,https" } else { "=https" };
    let max_time = if max_bytes > 10_000_000 || upload.is_some() { "300" } else { "30" };
    let mut curl = curl_command(follow, protos, max_time, max_bytes, method, url);
    let mut child = curl.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("could not run curl ({e}) — is curl installed?"))?;
    child.stdin.take().unwrap().write_all(cfg.as_bytes()).map_err(|e| e.to_string())?;
    drop(cfg);
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    // curl failed: whatever status line was parsed (the last 302 of a redirect loop, the 200 of an answer
    // cut short) is not an answer, so the person gets the reason in words (CAL-030)
    match out.status.code() {
        Some(0) => {}
        Some(code) => return Err(curl_failure(code, max_bytes)),
        None => return Err("curl was stopped before it got an answer".into()),
    }
    // the answer can hold a token (the OAuth2 endpoint): wiped when this function is done
    let raw = Zeroizing::new(out.stdout);
    let stderr = || String::from_utf8_lossy(&out.stderr).trim().to_string();
    let Some(pos) = raw.windows(MARK.len()).rposition(|w| w == MARK.as_bytes()) else {
        let err = stderr();
        return Err(if err.is_empty() { "no answer from the server".into() } else { err });
    };
    let tail = String::from_utf8_lossy(&raw[pos + MARK.len()..]).into_owned();
    let mut fields = tail.splitn(3, '\t');
    let (code, header_len, effective) = (fields.next().unwrap_or(""), fields.next().unwrap_or(""), fields.next().unwrap_or(""));
    let status: u16 = code.trim().parse().unwrap_or(0);
    let header_len = header_len.trim().parse::<usize>().unwrap_or(0).min(pos);
    let head = String::from_utf8_lossy(&raw[..header_len]).into_owned();
    let etag = last_header(&head, "etag").unwrap_or_default();
    let content_type = last_header(&head, "content-type").map(|c| c.split(';').next().unwrap_or("").trim().to_ascii_lowercase()).filter(|c| !c.is_empty() && !has_control(c));
    let body = raw[header_len..pos].to_vec();
    if status == 0 {
        let err = stderr();
        return Err(if err.is_empty() { "could not reach the server".into() } else { err });
    }
    if body.len() as u64 > max_bytes {
        return Err(too_big(max_bytes));
    }
    // older curl (7.x) puts the request's user name and password into the final address after a
    // redirect: never check, compare or return them
    let effective = if effective.trim().is_empty() { url.to_string() } else { without_user_info(effective.trim()) };
    if effective != url {
        check_url(&effective).map_err(|e| format!("the server redirected to an address that is not allowed: {e}"))?;
        if with_auth && !same_server(&effective, url) {
            return Err(format!("the server sent the request on to {}; Shale does not send your access token to another server", host_of(&effective)));
        }
    }
    Ok(BytesResponse { status, body, url: effective, etag: clean_etag(&etag), content_type })
}

/// The words for a curl exit code other than 0 (CAL-030). Unknown codes say the number, so a report can name it.
fn curl_failure(code: i32, max_bytes: u64) -> String {
    match code {
        1 => "the server sent the request on to a kind of address that is not allowed (for example from https to plain http)".into(),
        6 => "the server's name could not be found — check the address and your network".into(),
        7 => "could not connect to the server — it is down, the address or port is wrong, or a firewall is in the way".into(),
        18 => "the answer was cut short — the server stopped sending before it was complete".into(),
        28 => "the server took too long to answer".into(),
        35 | 60 => "the secure connection failed — the server's certificate is not trusted or does not match the address".into(),
        47 => "the server sent the request on too many times (a redirect loop) — check the address".into(),
        52 => "the server closed the connection without an answer".into(),
        56 => "the connection broke while the answer was coming in".into(),
        63 => too_big(max_bytes),
        c => format!("the connection to the server failed (curl error {c})"),
    }
}

/// The address without a `user:password@` part in its authority.
fn without_user_info(url: &str) -> String {
    let Some(start) = url.find("://").map(|i| i + 3) else { return url.to_string() };
    let end = url[start..].find(['/', '?', '#']).map_or(url.len(), |i| start + i);
    match url[start..end].rfind('@') {
        Some(at) => format!("{}{}", &url[..start], &url[start + at + 1..]),
        None => url.to_string(),
    }
}

fn too_big(max_bytes: u64) -> String {
    if max_bytes >= 1_000_000 {
        format!("the answer is bigger than {} MB", max_bytes / 1_000_000)
    } else {
        format!("the answer is bigger than {max_bytes} bytes")
    }
}

/// A header of the last response in curl's `-i` output (after redirects and "100 Continue").
fn last_header(headers: &str, name: &str) -> Option<String> {
    let block = headers.replace("\r\n", "\n");
    let last = block.split("\n\n").filter(|b| !b.trim().is_empty()).last()?;
    last.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
    })
}

fn check(r: &Response, requested: &str) -> Result<(), String> {
    match r.status {
        200..=299 => Ok(()),
        401 if !same_server(&r.url, requested) => Err(format!("the server sent the request on to {}; Shale does not send your password to another server — enter that address instead", host_of(&r.url))),
        401 | 403 => Err("the server refused the user name or password".into()),
        404 => Err("nothing found at this address (404)".into()),
        s => Err(format!("the server answered with status {s}")),
    }
}

// ---------------------------------------------------------------- XML

const DAV: &str = "DAV:";
const CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
const CS: &str = "http://calendarserver.org/ns/";
const APPLE: &str = "http://apple.com/ns/ical/";

/// Parse a server answer. Document type declarations (and so entity tricks) and a nesting beyond the limit of DEV-038 are refused.
fn parse(xml: &str) -> Result<Document<'_>, String> {
    crate::xml_limits::parse(xml).map_err(|e| format!("the server's answer could not be read ({e})"))
}

fn is_el(n: &Node, ns: &str, name: &str) -> bool {
    n.is_element() && n.tag_name().name() == name && n.tag_name().namespace() == Some(ns)
}

fn child<'a, 'i>(n: Node<'a, 'i>, ns: &str, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| is_el(c, ns, name))
}

fn text_of(n: Node) -> String {
    n.descendants().filter(|d| d.is_text()).filter_map(|d| d.text()).collect()
}

fn status_ok(s: &str) -> bool {
    s.split_whitespace().nth(1).map(|c| c.starts_with('2')).unwrap_or(false)
}

/// One `<response>` of a multistatus answer: its href and the properties the server found.
struct Resp<'a, 'i> {
    href: String,
    /// the `<prop>` elements of the propstats with a 2xx status
    props: Vec<Node<'a, 'i>>,
}

impl<'a, 'i> Resp<'a, 'i> {
    fn prop(&self, ns: &str, name: &str) -> Option<Node<'a, 'i>> {
        self.props.iter().find_map(|p| child(*p, ns, name))
    }
    fn prop_text(&self, ns: &str, name: &str) -> Option<String> {
        self.prop(ns, name).map(|n| text_of(n).trim().to_string()).filter(|t| !t.is_empty())
    }
    /// The href inside a property (current-user-principal, calendar-home-set).
    fn prop_href(&self, ns: &str, name: &str) -> Option<String> {
        self.prop(ns, name).and_then(|n| n.descendants().find(|d| is_el(d, DAV, "href"))).map(|h| text_of(h).trim().to_string()).filter(|h| !h.is_empty())
    }
    fn is_calendar(&self) -> bool {
        self.prop(DAV, "resourcetype").map(|rt| child(rt, CALDAV, "calendar").is_some()).unwrap_or(false)
    }
    /// Can hold events (task-only lists can't).
    fn holds_events(&self) -> bool {
        match self.prop(CALDAV, "supported-calendar-component-set") {
            None => true,
            Some(set) => {
                let comps: Vec<Node> = set.children().filter(|c| is_el(c, CALDAV, "comp")).collect();
                comps.is_empty() || comps.iter().any(|c| c.attribute("name").map(|n| n.eq_ignore_ascii_case("VEVENT")).unwrap_or(false))
            }
        }
    }
    /// From current-user-privilege-set; None when the server didn't say.
    fn read_only(&self) -> Option<bool> {
        self.prop(DAV, "current-user-privilege-set").map(|set| !set.descendants().any(|d| is_el(&d, DAV, "write") || is_el(&d, DAV, "write-content") || is_el(&d, DAV, "all")))
    }
}

fn responses<'a, 'i>(doc: &'a Document<'i>) -> Vec<Resp<'a, 'i>> {
    doc.descendants()
        .filter(|n| is_el(n, DAV, "response"))
        .map(|r| {
            let href = child(r, DAV, "href").map(text_of).unwrap_or_default().trim().to_string();
            let props = r.children().filter(|c| is_el(c, DAV, "propstat")).filter(|ps| child(*ps, DAV, "status").map(|s| status_ok(&text_of(s))).unwrap_or(true)).filter_map(|ps| child(ps, DAV, "prop")).collect();
            Resp { href, props }
        })
        .collect()
}

// ---------------------------------------------------------------- CalDAV

const CAL_PROPS: &str = r#"<d:resourcetype/><d:displayname/><a:calendar-color/><c:supported-calendar-component-set/><d:current-user-privilege-set/>"#;
const NS: &str = r#"xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/" xmlns:cs="http://calendarserver.org/ns/""#;

fn propfind_body(props: &str) -> String {
    format!(r#"<?xml version="1.0" encoding="utf-8"?><d:propfind {NS}><d:prop>{props}</d:prop></d:propfind>"#)
}

/// Calendars in a PROPFIND answer, and how many were left out because their address isn't allowed.
fn calendars_in(resps: &[Resp], base: &str) -> (Vec<FoundCalendar>, usize) {
    let mut refused = 0;
    let mut out = Vec::new();
    for r in resps.iter().filter(|r| r.is_calendar() && r.holds_events() && !r.href.is_empty()) {
        let url = resolve(base, &r.href);
        if check_url(&url).is_err() {
            refused += 1;
            continue;
        }
        let name = r.prop_text(DAV, "displayname").unwrap_or_else(|| url.trim_end_matches('/').rsplit('/').next().unwrap_or("Calendar").to_string());
        let color = r.prop_text(APPLE, "calendar-color").filter(|c| c.len() >= 7 && c.starts_with('#') && c[1..7].chars().all(|h| h.is_ascii_hexdigit())).map(|c| c[..7].to_string());
        out.push(FoundCalendar { url, name, color, read_only: r.read_only().unwrap_or(false) });
    }
    (out, refused)
}

fn is_2xx(s: u16) -> bool {
    (200..300).contains(&s)
}

/// For discovery only: some hosted services send the account on to another server. curl doesn't
/// send the password along such a redirect (right), so ask the final address directly — but
/// only over https. The calendars found there show their server in the dialog before adding.
fn propfind_following(url: &str, depth: &str, body: &str, acct: &Account) -> Result<Response, String> {
    let r = request("PROPFIND", url, depth, body, acct)?;
    if r.status == 401 && !same_server(&r.url, url) && r.url.to_ascii_lowercase().starts_with("https://") {
        let next = r.url.clone();
        return request("PROPFIND", &next, depth, body, acct);
    }
    Ok(r)
}

/// The answer says something about calendars (a calendar, the home or the principal).
fn useful(body: &str) -> bool {
    parse(body).map(|d| responses(&d).iter().any(|r| r.is_calendar() || r.prop_href(CALDAV, "calendar-home-set").is_some() || r.prop_href(DAV, "current-user-principal").is_some())).unwrap_or(false)
}

/// What a load or a discovery that Cancel stopped reports instead of a result (JOB-033, JOB-034).
pub const STOPPED: &str = "stopped";

/// A safe point of a discovery: between two requests, never inside one.
fn go_on(cancel: &Cancel) -> Result<(), String> {
    if cancel.is_cancelled() {
        Err(STOPPED.into())
    } else {
        Ok(())
    }
}

/// Find the calendars of an account. The address may be a calendar, the calendar home, the
/// principal, or just the server (then `/.well-known/caldav` is tried).
#[cfg(test)]
pub fn discover(acct: &Account) -> Result<Vec<FoundCalendar>, String> {
    discover_until(acct, &Cancel::default())
}

/// [`discover`] that stops between two requests when `cancel` is set, with the error [`STOPPED`] (JOB-034).
pub fn discover_until(acct: &Account, cancel: &Cancel) -> Result<Vec<FoundCalendar>, String> {
    let _flow = tracing::info_span!("flow.caldav_discover", peer = "calendar").entered();
    let entered = acct.url.trim();
    if entered.is_empty() {
        return Err("enter the server address".into());
    }
    check_url(entered)?;
    let first_props = propfind_body(&format!("<d:current-user-principal/><c:calendar-home-set/>{CAL_PROPS}"));
    go_on(cancel)?;
    let first = tracing::info_span!("flow.caldav_discover.probe", peer = "caldav").in_scope(|| propfind_following(entered, "0", &first_props, acct))?;
    let r = if matches!(first.status, 404 | 405) || (is_2xx(first.status) && !useful(&first.body)) {
        go_on(cancel)?;
        let wk = tracing::info_span!("flow.caldav_discover.well_known", peer = "caldav").in_scope(|| propfind_following(&resolve(entered, "/.well-known/caldav"), "0", &first_props, acct))?;
        if is_2xx(wk.status) || !is_2xx(first.status) {
            wk
        } else {
            first
        }
    } else {
        first
    };
    check(&r, entered)?;
    let doc = parse(&r.body)?;
    let resps = responses(&doc);
    let (found, mut refused) = calendars_in(&resps, &r.url);
    if !found.is_empty() {
        return Ok(found);
    }
    // calendar home: directly, or via the principal
    let mut home = resps.iter().find_map(|x| x.prop_href(CALDAV, "calendar-home-set")).map(|h| resolve(&r.url, &h));
    if home.is_none() {
        if let Some(p) = resps.iter().find_map(|x| x.prop_href(DAV, "current-user-principal")) {
            let purl = resolve(&r.url, &p);
            go_on(cancel)?;
            let pr = tracing::info_span!("flow.caldav_discover.principal", peer = "caldav").in_scope(|| propfind_following(&purl, "0", &propfind_body("<c:calendar-home-set/>"), acct))?;
            check(&pr, &purl)?;
            let pdoc = parse(&pr.body)?;
            home = responses(&pdoc).iter().find_map(|x| x.prop_href(CALDAV, "calendar-home-set")).map(|h| resolve(&pr.url, &h));
        }
    }
    let home = home.unwrap_or_else(|| r.url.clone());
    go_on(cancel)?;
    let hr = tracing::info_span!("flow.caldav_discover.calendars", peer = "caldav").in_scope(|| propfind_following(&home, "1", &propfind_body(CAL_PROPS), acct))?;
    check(&hr, &home)?;
    let hdoc = parse(&hr.body)?;
    let (found, refused2) = calendars_in(&responses(&hdoc), &hr.url);
    refused += refused2;
    if !found.is_empty() {
        Ok(found)
    } else if refused > 0 {
        Err("the server only offered calendars at unencrypted http:// addresses, which Shale refuses".into())
    } else {
        Err("no calendars with events found for this account".into())
    }
}

/// What a calendar says about itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalState {
    /// changes whenever anything in the calendar changes (getctag or sync-token)
    pub tag: Option<String>,
    pub read_only: Option<bool>,
}

/// The calendar's version tag and write permission (one small PROPFIND).
pub fn calendar_state(cal_url: &str, acct: &Account) -> Result<CalState, String> {
    let r = request("PROPFIND", cal_url, "0", &propfind_body("<cs:getctag/><d:sync-token/><d:current-user-privilege-set/>"), acct)?;
    check(&r, cal_url)?;
    let doc = parse(&r.body)?;
    let resps = responses(&doc);
    let Some(me) = resps.first() else { return Ok(CalState::default()) };
    let tag = me.prop_text(CS, "getctag").map(|t| format!("ctag:{t}")).or_else(|| me.prop_text(DAV, "sync-token").map(|t| format!("sync:{t}")));
    Ok(CalState { tag, read_only: me.read_only() })
}

fn utc_stamp(t: i64) -> String {
    let (y, m, d) = crate::calendar::time::civil_from_days(t.div_euclid(86_400));
    let s = t.rem_euclid(86_400);
    format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z", s / 3600, s % 3600 / 60, s % 60)
}

/// Events of one calendar between two UTC times. Events whose server address is on another
/// server than the calendar are shown but can't be changed (no address is kept for them).
pub fn fetch_events(cal_url: &str, acct: &Account, from: i64, to: i64) -> Result<Vec<Event>, String> {
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"><c:time-range start="{}" end="{}"/></c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"#,
        utc_stamp(from),
        utc_stamp(to)
    );
    let r = request("REPORT", cal_url, "1", &body, acct)?;
    check(&r, cal_url)?;
    let doc = parse(&r.body)?;
    let mut out = Vec::new();
    for resp in responses(&doc) {
        let href = Some(resolve(&r.url, &resp.href)).filter(|h| !resp.href.is_empty() && same_server(h, cal_url));
        let etag = href.as_ref().and_then(|_| resp.prop_text(DAV, "getetag")).and_then(|e| clean_etag(&e));
        if let Some(data) = resp.prop(CALDAV, "calendar-data") {
            for mut e in ics::parse_ics(&text_of(data)) {
                e.href = href.clone();
                e.etag = etag.clone();
                out.push(e);
            }
        }
    }
    Ok(out)
}

fn fnv64(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3))
}

/// A file name for an event on the server, from its UID. UIDs that aren't plain file names get
/// a hash of the full UID, so two different UIDs never end up in the same file.
fn event_file(uid: &str) -> String {
    let plain = !uid.is_empty() && uid.len() <= 100 && !uid.starts_with('.') && uid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if plain {
        return format!("{uid}.ics");
    }
    let clean: String = uid.chars().take(60).map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("{clean}-{:016x}.ics", fnv64(uid))
}

fn write_error(status: u16, creating: bool) -> String {
    match status {
        401 => "the server refused the user name or password".into(),
        403 => "you don't have permission to write to this calendar".into(),
        405 | 501 => "this calendar is read-only".into(),
        412 if creating => "an event with the same ID already exists in this calendar".into(),
        412 => CHANGED_MEANWHILE.into(),
        415 => "this calendar doesn't accept events".into(),
        507 => "the server has no space left".into(),
        s => format!("the server answered with status {s}"),
    }
}

/// The current version tag of one event on the server (None if the server doesn't say).
fn current_etag(href: &str, acct: &Account) -> Result<Option<String>, String> {
    let r = request("PROPFIND", href, "0", &propfind_body("<d:getetag/>"), acct)?;
    if !is_2xx(r.status) {
        return Ok(None);
    }
    let doc = parse(&r.body)?;
    let etag = responses(&doc).first().and_then(|x| x.prop_text(DAV, "getetag")).and_then(|e| clean_etag(&e));
    Ok(etag)
}

/// The lines of each event, sorted, so two copies of an event compare without regard to order.
fn src_key(events: &[Event]) -> Vec<Vec<String>> {
    let mut k: Vec<Vec<String>> = events
        .iter()
        .map(|e| {
            let mut l = e.src.clone();
            l.sort();
            l
        })
        .collect();
    k.sort();
    k
}

/// When no version tag is known: fetch the event and check that the server still has what Shale
/// read (`before`). Ok(true): same. Ok(false): changed, or gone (404).
fn unchanged_on_server(href: &str, acct: &Account, before: &[Event]) -> Result<bool, String> {
    let r = request_with("GET", href, &[], "", acct)?;
    match r.status {
        200..=299 => Ok(src_key(&ics::parse_ics(&r.body)) == src_key(before)),
        404 | 410 => Ok(false),
        401 => Err(check(&r, href).unwrap_err()),
        s => Err(format!("could not check the event on the server first: it answered with status {s}")),
    }
}

const CHANGED_MEANWHILE: &str = "the event was changed on the server meanwhile; refresh the calendar and try again";

/// Save an event to a calendar: `parts` is the event and, for a repeating one, its changed
/// single dates (all stored together in one file on the server). `existing`: the current
/// address and version tag when it is changed in place; None creates it.
/// `before`: the event's parts as Shale read them; without a version tag they are compared with
/// what the server has now, and a difference stops the save (CAL-016).
/// Returns the address and the new version tag (if the server sent one).
pub fn put_event(cal_url: &str, acct: &Account, parts: &[Event], existing: Option<(&str, Option<&str>)>, before: &[Event]) -> Result<(String, Option<String>), String> {
    let e = parts.first().ok_or("nothing to save")?;
    let _flow = tracing::info_span!("flow.caldav_put", peer = "calendar").entered();
    let clean: Vec<Event> = parts.iter().map(|p| Event { href: None, etag: None, ..p.clone() }).collect();
    let body = ics::to_ics("", &clean);
    let creating = existing.is_none();
    let (url, tag) = match existing {
        Some((href, tag)) => {
            if !same_server(href, cal_url) {
                return Err(OTHER_SERVER.into());
            }
            // without a known version, ask for it and check the server still has what was read:
            // never overwrite a newer version blindly
            let tag = match tag.and_then(clean_etag) {
                Some(t) => Some(t),
                None => {
                    let now = tracing::info_span!("flow.caldav_put.fetch_etag", peer = "caldav").in_scope(|| current_etag(href, acct)).ok().flatten();
                    if !tracing::info_span!("flow.caldav_put.check_server", peer = "caldav").in_scope(|| unchanged_on_server(href, acct, before))? {
                        return Err(CHANGED_MEANWHILE.into());
                    }
                    now
                }
            };
            (href.to_string(), tag)
        }
        None => (resolve(&format!("{}/", cal_url.trim_end_matches('/')), &event_file(&e.uid)), None),
    };
    let cond = match (creating, &tag) {
        (true, _) => Some("If-None-Match: *".to_string()),
        (false, Some(t)) => Some(format!("If-Match: {t}")),
        (false, None) => None,
    };
    let mut headers = vec!["Content-Type: text/calendar; charset=utf-8"];
    if let Some(c) = &cond {
        headers.push(c);
    }
    let r = tracing::info_span!("flow.caldav_put.put", peer = "caldav").in_scope(|| request_with("PUT", &url, &headers, &body, acct))?;
    match r.status {
        200..=299 => Ok((url, r.etag)),
        401 => Err(check(&r, &url).unwrap_err()),
        s => Err(write_error(s, creating)),
    }
}

/// Remove an event from the server. An event that is already gone counts as removed.
/// `acct.url` is the calendar's address; the event must be on the same server.
/// Without a version tag the event is first compared with `before` (as Shale read it), like in
/// `put_event`; one that was changed on the server is not deleted.
pub fn delete_event(href: &str, acct: &Account, etag: Option<&str>, before: &[Event]) -> Result<(), String> {
    if !same_server(href, &acct.url) {
        return Err(OTHER_SERVER.into());
    }
    let _flow = tracing::info_span!("flow.caldav_delete", peer = "calendar").entered();
    let tag = etag.and_then(clean_etag);
    if tag.is_none() {
        let r = tracing::info_span!("flow.caldav_delete.check_server", peer = "caldav").in_scope(|| request_with("GET", href, &[], "", acct))?;
        match r.status {
            404 | 410 => return Ok(()),
            200..=299 if src_key(&ics::parse_ics(&r.body)) != src_key(before) => return Err(CHANGED_MEANWHILE.into()),
            200..=299 => {}
            401 => return Err(check(&r, href).unwrap_err()),
            s => return Err(format!("could not check the event on the server first: it answered with status {s}")),
        }
    }
    let cond = tag.map(|t| format!("If-Match: {t}"));
    let headers: Vec<&str> = cond.iter().map(|s| s.as_str()).collect();
    let r = tracing::info_span!("flow.caldav_delete.delete", peer = "caldav").in_scope(|| request_with("DELETE", href, &headers, "", acct))?;
    match r.status {
        200..=299 | 404 | 410 => Ok(()),
        401 => Err(check(&r, href).unwrap_err()),
        s => Err(write_error(s, false)),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // spec: CAL-030
    #[test]
    fn curl_exit_codes_have_words() {
        for (code, words) in [
            (1, "kind of address that is not allowed"),
            (6, "name could not be found"),
            (7, "could not connect"),
            (18, "cut short"),
            (28, "took too long"),
            (35, "certificate is not trusted"),
            (60, "certificate is not trusted"),
            (47, "redirect loop"),
            (52, "closed the connection without an answer"),
            (56, "connection broke"),
            (63, "bigger than 1000 bytes"),
            (99, "curl error 99"),
        ] {
            let text = curl_failure(code, 1000);
            assert!(text.contains(words), "{code}: {text}");
            assert!(!text.contains("status"), "{code}: {text}");
        }
        assert!(curl_failure(63, 20_000_000).contains("20 MB"));
    }

    // spec: CAL-019
    #[test]
    fn the_final_address_after_a_redirect_never_keeps_a_user_name_or_password() {
        // older curl reports it like this after a redirect (raba/shale#104)
        assert_eq!(without_user_info("https://ada:s3cret@dav.example.com/cal/"), "https://dav.example.com/cal/");
        assert_eq!(without_user_info("https://ada@dav.example.com:8443/x?a=b@c"), "https://dav.example.com:8443/x?a=b@c");
        assert_eq!(without_user_info("http://127.0.0.1:5232/p@q/"), "http://127.0.0.1:5232/p@q/", "an @ in the path stays");
        assert_eq!(without_user_info("https://dav.example.com"), "https://dav.example.com");
        assert_eq!(without_user_info("not a url"), "not a url");
    }

    // spec: SEC-017
    #[test]
    fn curl_config_strings_are_escaped() {
        let mut s = String::new();
        quote_into(&mut s, &["a\"b", ":", "c\\d\r\n\te"]);
        assert_eq!(s, "\"a\\\"b:c\\\\d\\r\\n\\te\"");
    }

    // spec: SEC-002
    #[test]
    fn the_curl_command_has_no_secret_in_it_and_ignores_the_curlrc() {
        let cmd = curl_command(true, "=https", "30", 1000, "PROPFIND", "https://dav.example.com/cal/");
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args.first().map(String::as_str), Some("-q"), "{args:?}");
        let k = args.iter().position(|a| a == "-K").expect("the configuration is read with -K");
        assert_eq!(args[k + 1], "-", "{args:?}");
        for banned in ["--user", "-u", "--data", "-d", "--data-binary", "-H", "--header", "--form", "-F"] {
            assert!(!args.iter().any(|a| a == banned), "{banned} is on the command line: {args:?}");
        }
        // nothing but fixed words, numbers and the method and address is on the line
        assert!(args.iter().all(|a| !a.contains('@') && !a.contains("pw")), "{args:?}");
        assert_eq!(args.last().map(String::as_str), Some("https://dav.example.com/cal/"));
    }

    // spec: SEC-002
    #[cfg(target_os = "linux")]
    #[test]
    fn while_curl_runs_the_password_is_on_no_command_line_of_the_system() {
        if !has_curl() {
            return;
        }
        let secret = "pw-never-on-a-command-line-4711";
        let acct = Account { url: String::new(), username: "ada-never-on-a-command-line".into(), password: secret.into() };
        let (base, h) = mock_http(1, move |_, _, headers, body| {
            assert!(headers.to_ascii_lowercase().contains("authorization: basic"), "{headers}");
            // curl waits for this answer: look at every command line on this computer now
            let mut curls = 0;
            for entry in std::fs::read_dir("/proc").unwrap().flatten() {
                let Ok(raw) = std::fs::read(entry.path().join("cmdline")) else { continue };
                let line = String::from_utf8_lossy(&raw).replace('\0', " ");
                if line.contains("-K - ") && line.contains("--proto") {
                    curls += 1;
                    assert!(line.starts_with("curl -q ") || line.contains("/curl -q "), "{line}");
                    assert!(!line.contains(secret) && !line.contains("never-on-a-command-line") && !line.contains("BODYTEXT"), "{line}");
                }
            }
            assert!(curls >= 1, "the running curl was not found");
            assert_eq!(body, "BODYTEXT");
            (200, String::new())
        });
        let r = request("PUT", &format!("{base}/x"), "0", "BODYTEXT", &acct).unwrap();
        assert_eq!(r.status, 200);
        h.join().unwrap();
    }
    use std::io::{BufRead, BufReader, Read};
    use std::net::TcpListener;
    use std::process::Command;

    fn has_curl() -> bool {
        Command::new("curl").arg("--version").output().is_ok()
    }

    // spec: SEC-001, SEC-004
    #[test]
    fn only_https_or_this_computer() {
        for ok in ["https://dav.example.com/cal/", "HTTPS://Dav.Example.com:8443/x", "http://localhost:5232/", "http://127.0.0.1:1/a", "http://[::1]:8080/", "http://radicale.localhost/"] {
            assert!(check_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://dav.example.com/",
            "http://127.0.0.1.evil.com/",
            "http://localhost.evil.com/",
            "ftp://x/",
            "file:///etc/passwd",
            "https://ada:pw@dav.example.com/",
            "https://dav.example.com/a b",
            "https://dav.example.com/\r\nX: y",
            "-K/etc/passwd",
            "https:///x",
        ] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
        assert!(same_server("https://H.example/a", "https://h.example:443/b"));
        assert!(!same_server("https://h.example/a", "http://h.example/a"));
        assert!(!same_server("https://h.example/a", "https://h.example:8443/a"));
        assert!(!same_server("https://h.example/a", "https://other.example/a"));
        // refused before curl even runs
        let acct = Account { url: String::new(), username: "ada".into(), password: "pw".into() };
        let err = request("PROPFIND", "http://dav.example.com/", "0", "", &acct).unwrap_err();
        assert!(err.contains("https"), "{err}");
    }

    // spec: SEC-001
    #[test]
    fn headers_with_line_breaks_are_refused() {
        let acct = Account { url: String::new(), username: "ada".into(), password: "pw".into() };
        let err = request_with("PUT", "http://127.0.0.1:1/x.ics", &["If-Match: \"a\"\r\nX-Evil: 1"], "x", &acct).unwrap_err();
        assert!(err.contains("If-Match"), "{err}");
        assert_eq!(clean_etag("\"a\"\r\nX: y"), None);
        assert_eq!(clean_etag(" \"a\" "), Some("\"a\"".into()));
        let bad_user = Account { username: "a:b".into(), ..acct };
        assert!(request("PROPFIND", "http://127.0.0.1:1/", "0", "", &bad_user).is_err());
    }

    // spec: SEC-007, SEC-016
    #[test]
    fn passwords_stay_out_of_debug_output() {
        let acct = Account { url: "https://x".into(), username: "ada".into(), password: "hunter2".into() };
        let s = format!("{acct:?}");
        assert!(!s.contains("hunter2") && s.contains("***"), "{s}");
    }

    // spec: SEC-001
    #[test]
    fn xml_and_addresses() {
        let x = r#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/cal/work/</d:href><d:propstat><d:prop><d:displayname>Work &amp; more</d:displayname><d:resourcetype><d:collection/><cal:calendar xmlns:cal="urn:ietf:params:xml:ns:caldav"/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response><d:response><d:href>/cal/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response></d:multistatus>"#;
        let doc = parse(x).unwrap();
        let (found, _) = calendars_in(&responses(&doc), "https://example.com/dav/");
        assert_eq!(found, vec![FoundCalendar { url: "https://example.com/cal/work/".into(), name: "Work & more".into(), color: None, read_only: false }]);
        assert_eq!(resolve("https://h.example/a/b/", "c/"), "https://h.example/a/b/c/");
        assert_eq!(resolve("https://h.example/a/b/", "/x/"), "https://h.example/x/");
        assert_eq!(resolve("https://h.example", "http://o.example/y"), "http://o.example/y");
        assert_eq!(resolve("https://h.example/a/", "//o.example/y"), "https://o.example/y");
        assert_eq!(resolve("https://h.example/a/", "file:///etc/passwd"), "file:///etc/passwd");
        // a "calendar" element in another namespace, and a DTD, are not taken
        assert!(parse(r#"<!DOCTYPE x [<!ENTITY a "b">]><x>&a;</x>"#).is_err());
        let other_ns = r#"<d:multistatus xmlns:d="DAV:" xmlns:o="urn:other"><d:response><d:href>/o/</d:href><d:propstat><d:prop><d:resourcetype><o:calendar/></d:resourcetype></d:prop></d:propstat></d:response></d:multistatus>"#;
        assert!(calendars_in(&responses(&parse(other_ns).unwrap()), "https://h.example/").0.is_empty());
    }

    // spec: DEV-038
    #[test]
    fn a_server_answer_nested_too_deeply_is_an_error() {
        let deep = format!("<d:multistatus xmlns:d=\"DAV:\">{}{}</d:multistatus>", "<d:response>".repeat(5000), "</d:response>".repeat(5000));
        let e = parse(&deep).unwrap_err();
        assert!(e.starts_with("the server's answer could not be read (the XML is nested too deeply"), "{e}");
    }

    // spec: CAL-002
    #[test]
    fn task_lists_and_read_only_calendars() {
        let x = r##"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/">
          <d:response><d:href>/cal/tasks/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><c:supported-calendar-component-set><c:comp name="VTODO"/></c:supported-calendar-component-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
          <d:response><d:href>/cal/holidays/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><c:supported-calendar-component-set><c:comp name="VEVENT"/></c:supported-calendar-component-set><d:current-user-privilege-set><d:privilege><d:read/></d:privilege></d:current-user-privilege-set><a:calendar-color>#ff00aaff</a:calendar-color></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat><d:propstat><d:prop><d:displayname/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat></d:response>
          <d:response><d:href>/cal/work/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><d:current-user-privilege-set><d:privilege><d:read/></d:privilege><d:privilege><d:write/></d:privilege></d:current-user-privilege-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
          <d:response><d:href>http://evil.example/cal/x/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype></d:prop></d:propstat></d:response>
        </d:multistatus>"##;
        let doc = parse(x).unwrap();
        let (found, refused) = calendars_in(&responses(&doc), "https://h.example/cal/");
        assert_eq!(refused, 1, "the http:// calendar elsewhere is left out");
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!((found[0].name.as_str(), found[0].read_only, found[0].color.as_deref()), ("holidays", true, Some("#ff00aa")));
        assert_eq!((found[1].name.as_str(), found[1].read_only), ("work", false));
    }

    // spec: SEC-008
    #[test]
    fn event_files_never_collide() {
        assert_eq!(event_file("abc-123@x"), event_file("abc-123@x"));
        assert_eq!(event_file("plain-uid_1.2"), "plain-uid_1.2.ics");
        assert_ne!(event_file("ev/1"), event_file("ev_1"));
        assert_ne!(event_file("a/b"), event_file("a?b"));
        assert!(!event_file("../../x").contains('/'));
        assert!(!event_file("..").starts_with(".."));
    }

    /// A loopback port that nothing listens on and that stays this test's own while the value
    /// lives, for a test of an unreachable server (a refused connection).
    ///
    /// Never make such a port by binding `127.0.0.1:0`, reading the port and dropping the
    /// listener: the kernel hands a released port out again at once (about 3 in 100 binds in a
    /// row get a port of the last 50), so under load another test's pretend server, or a
    /// process of the machine, is given the port between the drop and the connect, and the
    /// "unreachable" server answers (or takes one of the connections a `mock_http_with` counts).
    /// Hold the value until the connection attempt is over. DEV-016.
    ///
    /// A connect to the port is refused: the peer answers with an RST, so the error is "refused",
    /// never a timeout. No other bind gets the port while it is held. How the port is held
    /// differs, because the systems handle a socket that is bound but does not listen differently:
    ///
    /// - Linux and Windows: a socket that is bound to 127.0.0.1 and does not listen. Neither
    ///   system finds it for an incoming SYN (only listening and connected sockets are looked up),
    ///   so the SYN gets an RST. It blocks every other bind of the port, also one with
    ///   SO_REUSEADDR (which `TcpListener::bind` sets on Unix): Linux because the held socket has
    ///   no SO_REUSEADDR, Windows because the address is in use. The local port of a connected
    ///   socket is not enough there: Windows lets a bind take a port only connected sockets use.
    /// - macOS (BSD): `tcp_input` does find a bound, non-listening socket for the SYN, sees it in
    ///   state CLOSED and drops the segment without an RST, so the connect times out (#302). The
    ///   port is the local port of a connected socket instead. A SYN to it matches no listener
    ///   and no connection (its foreign address is another one), so it gets an RST. A bind
    ///   without SO_REUSEADDR is refused: its wildcard lookup counts connected sockets. A bind
    ///   with SO_REUSEADDR looks only for sockets without a foreign address and could take the
    ///   port; the kernel's hand-out of free ports (`127.0.0.1:0`) gets to it again only after
    ///   going through the ephemeral range, unlike a dropped listener's port. No socket can both
    ///   be refused and keep a SO_REUSEADDR bind away on BSD: whatever that bind's lookup finds,
    ///   the SYN's lookup finds too, and it is either listening (accepts) or closed (drops).
    pub(crate) struct ClosedPort {
        pub(crate) port: u16,
        #[cfg(all(unix, not(target_vendor = "apple")))]
        _socket: std::os::fd::OwnedFd,
        #[cfg(target_vendor = "apple")]
        _client: std::net::TcpStream,
        #[cfg(target_vendor = "apple")]
        _listener: TcpListener,
        #[cfg(windows)]
        socket: windows_sys::Win32::Networking::WinSock::SOCKET,
    }

    /// A TCP socket bound to 127.0.0.1:`port` (0: a free port) without SO_REUSEADDR and not
    /// listening, and the port it got.
    #[cfg(unix)]
    fn bound_socket(port: u16) -> std::io::Result<(std::os::fd::OwnedFd, u16)> {
        use std::os::fd::{FromRawFd, OwnedFd};
        // SAFETY: a plain socket call; the result is checked below
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `fd` is a new, open descriptor that nothing else owns
        let socket = unsafe { OwnedFd::from_raw_fd(fd) };
        // SAFETY: an all-zero sockaddr_in is a valid value
        let mut addr: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        addr.sin_family = libc::AF_INET as libc::sa_family_t;
        addr.sin_port = port.to_be();
        addr.sin_addr.s_addr = u32::from_ne_bytes([127, 0, 0, 1]);
        let size = libc::socklen_t::try_from(std::mem::size_of::<libc::sockaddr_in>()).unwrap();
        #[cfg(target_vendor = "apple")]
        {
            addr.sin_len = u8::try_from(size).unwrap();
        }
        // SAFETY: `addr` is a valid sockaddr_in of the length given
        if unsafe { libc::bind(fd, std::ptr::addr_of!(addr).cast(), size) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut len = size;
        // SAFETY: `addr` and `len` are valid for writing, `len` is the size of `addr`
        if unsafe { libc::getsockname(fd, std::ptr::addr_of_mut!(addr).cast(), &mut len) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok((socket, u16::from_be(addr.sin_port)))
    }

    #[cfg(all(unix, not(target_vendor = "apple")))]
    pub(crate) fn closed_port() -> ClosedPort {
        // port 0 and getsockname: no bind-drop-rebind race
        let (socket, port) = bound_socket(0).unwrap();
        ClosedPort { port, _socket: socket }
    }

    #[cfg(target_vendor = "apple")]
    pub(crate) fn closed_port() -> ClosedPort {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        ClosedPort { port: client.local_addr().unwrap().port(), _client: client, _listener: listener }
    }

    #[cfg(windows)]
    pub(crate) fn closed_port() -> ClosedPort {
        use windows_sys::Win32::Networking::WinSock as ws;
        // a free port; creating a std socket also starts Winsock for the calls below
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut addr = ws::SOCKADDR_IN { sin_family: ws::AF_INET, sin_port: port.to_be(), ..Default::default() };
        addr.sin_addr.S_un.S_addr = u32::from_ne_bytes([127, 0, 0, 1]);
        // SAFETY: plain Winsock calls on a socket made here; `addr` is a valid SOCKADDR_IN of the length given
        let socket = unsafe { ws::socket(i32::from(ws::AF_INET), ws::SOCK_STREAM, ws::IPPROTO_TCP) };
        assert_ne!(socket, ws::INVALID_SOCKET, "socket");
        let size = i32::try_from(std::mem::size_of::<ws::SOCKADDR_IN>()).unwrap();
        // SAFETY: see above
        let bound = unsafe { ws::bind(socket, std::ptr::addr_of!(addr).cast(), size) };
        assert_eq!(bound, 0, "bind");
        ClosedPort { port, socket }
    }

    #[cfg(windows)]
    impl Drop for ClosedPort {
        fn drop(&mut self) {
            // SAFETY: the socket is ours and closed once
            unsafe { windows_sys::Win32::Networking::WinSock::closesocket(self.socket) };
        }
    }

    // spec: DEV-016
    #[test]
    fn a_closed_port_refuses_connections_and_is_not_handed_to_anybody_while_it_is_held() {
        let held = closed_port();
        let addr = ("127.0.0.1", held.port);
        let err = std::net::TcpStream::connect(addr).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::ConnectionRefused, "{err}");
        // a bind without SO_REUSEADDR (the kernel's own rule for a port in use) is refused everywhere
        #[cfg(unix)]
        assert!(bound_socket(held.port).is_err(), "a plain bind is not given the port");
        // and on Linux and Windows even std's bind, which sets SO_REUSEADDR on Unix (see
        // `ClosedPort` for why macOS cannot refuse that one and the connect too)
        #[cfg(not(target_vendor = "apple"))]
        assert!(TcpListener::bind(addr).is_err(), "no pretend server can be given the port");
    }

    /// A pretend HTTP server on localhost: `handler(method, path, headers, body) -> (status, body)`.
    /// Serves `n` requests. Returns the base URL and, when joined, "METHOD PATH" of every request.
    pub(crate) fn mock_http(n: usize, handler: impl Fn(&str, &str, &str, &str) -> (u16, String) + Send + 'static) -> (String, std::thread::JoinHandle<Vec<String>>) {
        mock_http_with(n, move |m, p, h, b| {
            let (s, body) = handler(m, p, h, b);
            (s, String::new(), body)
        })
    }

    // spec: SOC-053, SEC-026
    #[test]
    fn an_upload_names_its_file_to_curl_through_its_input_never_the_command_line_and_odd_names_are_refused() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("shale-upload-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("pic.bin");
        std::fs::write(&file, "FILEBYTES").unwrap();
        let nobody = Account { url: String::new(), username: String::new(), password: Default::default() };
        let (base, handle) = mock_http(2, |_, path, headers, body| {
            let low = headers.to_ascii_lowercase();
            assert!(low.contains("authorization: bearer tok-secret"), "{headers}");
            if path == "/raw" {
                assert!(low.contains("content-type: image/png") && body == "FILEBYTES", "{headers} {body}");
            } else {
                assert!(low.contains("multipart/form-data") && body.contains("name=\"file\"; filename=\"pic.bin\"") && body.contains("FILEBYTES") && body.contains("name=\"description\"") && body.contains("a \"quoted\" text"), "{body}");
            }
            (200, "{}".into())
        });
        let hs = ["Authorization: Bearer tok-secret", "Content-Type: image/png"];
        let r = request_upload("POST", &format!("{base}/raw"), &hs, Upload::Raw { path: &file }, &nobody, 1000).unwrap();
        assert_eq!(r.status, 200);
        let form = Upload::Form { field: "file", path: &file, mime: "image/png", fields: &[("description", "a \"quoted\" text")] };
        assert_eq!(request_upload("POST", &format!("{base}/form"), &hs[..1], form, &nobody, 1000).unwrap().status, 200);
        handle.join().unwrap();
        // a name or type that could change the meaning of curl's input is refused before curl runs
        for bad in ["a\"b.png", "a;b.png", "a,b.png", "a\nb.png"] {
            let p = dir.join(bad);
            assert!(request_upload("POST", &format!("{base}/raw"), &hs, Upload::Raw { path: &p }, &nobody, 1000).unwrap_err().contains("special characters"), "{bad}");
        }
        let odd_mime = Upload::Form { field: "file", path: &file, mime: "image/png;filename=x", fields: &[] };
        assert!(request_upload("POST", &format!("{base}/form"), &hs, odd_mime, &nobody, 1000).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: SEC-026
    #[test]
    fn a_windows_path_is_plain_for_curl_and_quoted_in_its_config_and_curl_syntax_is_not() {
        assert!(curl_plain(r"C:\Users\RUNNER~1\AppData\Local\Temp\shale-1234\pic.png"));
        assert!(curl_plain("image/png") && curl_plain("/tmp/shale x/pic.png"));
        for bad in ["a\"b.png", "a;b.png", "a,b.png", "a\nb.png", "a\rb", "image/png;filename=x", "a\0b"] {
            assert!(!curl_plain(bad), "{bad:?}");
        }
        // in the config the backslashes are doubled, so curl reads the path back unchanged
        let mut cfg = String::new();
        quote_into(&mut cfg, &["@", r"C:\Users\RUNNER~1\pic.png"]);
        assert_eq!(cfg, r#""@C:\\Users\\RUNNER~1\\pic.png""#);
    }

    /// Like `mock_http`, the handler also returns extra response header lines.
    pub(crate) fn mock_http_with(n: usize, handler: impl Fn(&str, &str, &str, &str) -> (u16, String, String) + Send + 'static) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let h = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for stream in l.incoming().take(n) {
                let mut s = stream.unwrap();
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut first = String::new();
                r.read_line(&mut first).unwrap();
                let mut len = 0usize;
                let mut headers = String::new();
                loop {
                    let mut h = String::new();
                    r.read_line(&mut h).unwrap();
                    if h.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                    headers.push_str(&h);
                }
                let mut body = vec![0u8; len];
                r.read_exact(&mut body).unwrap();
                let parts: Vec<&str> = first.split_whitespace().collect();
                let (method, path) = (parts[0].to_string(), parts[1].to_string());
                seen.push(format!("{method} {path}"));
                let (status, extra, reply) = handler(&method, &path, &headers, &String::from_utf8_lossy(&body));
                let resp = format!("HTTP/1.1 {status} X\r\nContent-Type: application/xml\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{reply}", reply.len());
                s.write_all(resp.as_bytes()).unwrap();
            }
            seen
        });
        (base, h)
    }

    /// A pretend CalDAV server on localhost for the tests.
    pub(crate) fn mock_server(event_ics: String) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let h = std::thread::spawn(move || {
            let mut seen = Vec::new();
            l.set_nonblocking(false).unwrap();
            for stream in l.incoming().take(4) {
                let mut s = stream.unwrap();
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut first = String::new();
                r.read_line(&mut first).unwrap();
                let mut len = 0usize;
                let mut auth = String::new();
                loop {
                    let mut h = String::new();
                    r.read_line(&mut h).unwrap();
                    if h.trim().is_empty() {
                        break;
                    }
                    let lower = h.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                    if lower.starts_with("authorization:") {
                        auth = h.trim().to_string();
                    }
                }
                let mut body = vec![0u8; len];
                r.read_exact(&mut body).unwrap();
                let parts: Vec<&str> = first.split_whitespace().collect();
                let (method, path) = (parts[0], parts[1]);
                seen.push(format!("{method} {path} {}", if auth.is_empty() { "no-auth" } else { "auth" }));
                let reply = match (method, path) {
                    ("PROPFIND", "/") => r#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/</d:href><d:propstat><d:prop><d:current-user-principal><d:href>/principals/ada/</d:href></d:current-user-principal><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response></d:multistatus>"#.to_string(),
                    ("PROPFIND", "/principals/ada/") => r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/principals/ada/</d:href><d:propstat><d:prop><c:calendar-home-set><d:href>/calendars/ada/</d:href></c:calendar-home-set></d:prop></d:propstat></d:response></d:multistatus>"#.to_string(),
                    ("PROPFIND", "/calendars/ada/") => r##"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/"><d:response><d:href>/calendars/ada/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response><d:response><d:href>/calendars/ada/work/</d:href><d:propstat><d:prop><d:displayname>Work</d:displayname><a:calendar-color>#1971c2ff</a:calendar-color><d:resourcetype><d:collection/><c:calendar/></d:resourcetype></d:prop></d:propstat></d:response></d:multistatus>"##.to_string(),
                    ("REPORT", "/calendars/ada/work/") => format!(r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/calendars/ada/work/1.ics</d:href><d:propstat><d:prop><c:calendar-data>{}</c:calendar-data></d:prop></d:propstat></d:response></d:multistatus>"#, event_ics.replace('&', "&amp;").replace('<', "&lt;")),
                    _ => String::new(),
                };
                let status = if reply.is_empty() { "404 Not Found" } else { "207 Multi-Status" };
                let resp = format!("HTTP/1.1 {status}\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
                s.write_all(resp.as_bytes()).unwrap();
            }
            seen
        });
        (base, h)
    }

    // spec: CAL-002, SEC-002
    #[test]
    fn discovers_and_fetches_from_a_server() {
        if !has_curl() {
            return;
        }
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:srv1\r\nDTSTART:20261113T090000Z\r\nDTEND:20261113T100000Z\r\nSUMMARY:Server meeting & co\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n".to_string();
        let (base, h) = mock_server(ics);
        let acct = Account { url: format!("{base}/"), username: "ada".into(), password: "pa\"ss".into() };
        let found = discover(&acct).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Work");
        assert_eq!(found[0].color.as_deref(), Some("#1971c2"));
        assert_eq!(found[0].url, format!("{base}/calendars/ada/work/"));
        let ev = fetch_events(&found[0].url, &acct, 0, i64::MAX / 4).unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].summary, "Server meeting & co");
        let seen = h.join().unwrap();
        assert!(seen.iter().all(|s| s.ends_with("auth")), "credentials were sent: {seen:?}");
        assert_eq!(seen.len(), 4);
    }

    // spec: CAL-004, CAL-011, SEC-004
    #[test]
    fn writes_and_removes_events() {
        if !has_curl() {
            return;
        }
        let file = event_file("ev/1");
        let f2 = file.clone();
        let (base, h) = mock_http_with(6, move |method, path, headers, body| {
            let lower = headers.to_ascii_lowercase();
            let ours = path == format!("/cal/rw/{f2}");
            match method {
                "PUT" if ours && lower.contains("if-none-match: *") && body.contains("SUMMARY:Lunch") => (201, "ETag: \"v2\"\r\n".into(), String::new()),
                "PUT" if ours && lower.contains("if-match: \"v1\"") => (412, String::new(), String::new()),
                "PUT" if path.starts_with("/cal/ro/") => (403, String::new(), String::new()),
                "GET" if ours => (200, String::new(), SERVER_ICS.into()),
                "GET" if path == "/cal/rw/gone.ics" => (404, String::new(), String::new()),
                "DELETE" if ours && !lower.contains("content-type") => (204, String::new(), String::new()),
                "DELETE" if path == "/cal/rw/gone.ics" => (404, String::new(), String::new()),
                _ => (500, String::new(), String::new()),
            }
        });
        let cal = format!("{base}/cal/rw");
        let acct = Account { url: cal.clone(), username: "ada".into(), password: "x".into() };
        let e = Event { uid: "ev/1".into(), summary: "Lunch".into(), start: 1_800_000_000, end: 1_800_001_800, ..Default::default() };
        let (href, etag) = put_event(&cal, &acct, std::slice::from_ref(&e), None, &[]).unwrap();
        assert_eq!(href, format!("{base}/cal/rw/{file}"));
        assert_eq!(etag.as_deref(), Some("\"v2\""), "the new version tag is kept");
        let err = put_event(&format!("{base}/cal/ro/"), &acct, std::slice::from_ref(&e), None, &[]).unwrap_err();
        assert!(err.contains("permission"), "{err}");
        let err = put_event(&cal, &acct, std::slice::from_ref(&e), Some((&href, Some("\"v1\""))), &[]).unwrap_err();
        assert!(err.contains("changed on the server"), "{err}");
        let before = ics::parse_ics(SERVER_ICS);
        delete_event(&href, &acct, None, &before).unwrap();
        delete_event(&format!("{base}/cal/rw/gone.ics"), &acct, None, &before).unwrap();
        // an event address on another server is never contacted
        let err = put_event(&cal, &acct, std::slice::from_ref(&e), Some(("https://evil.example/x.ics", Some("\"v1\""))), &[]).unwrap_err();
        assert!(err.contains("different server"), "{err}");
        let err = delete_event("https://evil.example/x.ics", &acct, None, &[]).unwrap_err();
        assert!(err.contains("different server"), "{err}");
        // a missing version tag is looked up before changing in place
        let (b2, h2) = mock_http(3, |method, _, headers, _| match method {
            "GET" => (200, SERVER_ICS.into()),
            "PROPFIND" => {
                (207, r#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/c/a.ics</d:href><d:propstat><d:prop><d:getetag>"now"</d:getetag></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#.into())
            }
            "PUT" if headers.to_ascii_lowercase().contains("if-match: \"now\"") => (204, String::new()),
            _ => (500, String::new()),
        });
        let acct2 = Account { url: format!("{b2}/c/"), ..acct.clone() };
        let before = ics::parse_ics(SERVER_ICS);
        put_event(&acct2.url, &acct2, std::slice::from_ref(&e), Some((&format!("{b2}/c/a.ics"), None)), &before).unwrap();
        assert_eq!(h2.join().unwrap(), vec!["PROPFIND /c/a.ics".to_string(), "GET /c/a.ics".into(), "PUT /c/a.ics".into()]);
        assert_eq!(h.join().unwrap().len(), 6);
    }

    const SERVER_ICS: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:ev/1\r\nDTSTART:20261113T090000Z\r\nSUMMARY:Lunch\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    // spec: CAL-016
    #[test]
    fn no_version_tag_means_compare_with_the_server_first() {
        if !has_curl() {
            return;
        }
        let before = ics::parse_ics(SERVER_ICS);
        let e = Event { summary: "Lunch at noon".into(), ..before[0].clone() };
        // the server still has what was read: saved
        let (b, h) = mock_http(3, |m, _, _, _| match m {
            "GET" => (200, SERVER_ICS.into()),
            "PUT" => (204, String::new()),
            _ => (404, String::new()),
        });
        let acct = Account { url: format!("{b}/c/"), username: String::new(), password: Default::default() };
        put_event(&acct.url, &acct, std::slice::from_ref(&e), Some((&format!("{b}/c/a.ics"), None)), &before).unwrap();
        assert_eq!(h.join().unwrap(), vec!["PROPFIND /c/a.ics".to_string(), "GET /c/a.ics".into(), "PUT /c/a.ics".into()]);
        // the server has something else now: nothing is sent
        let changed = SERVER_ICS.replace("Lunch", "Brunch");
        let (b, h) = mock_http(2, move |m, _, _, _| match m {
            "GET" => (200, changed.clone()),
            _ => (500, String::new()),
        });
        let acct = Account { url: format!("{b}/c/"), username: String::new(), password: Default::default() };
        let err = put_event(&acct.url, &acct, std::slice::from_ref(&e), Some((&format!("{b}/c/a.ics"), None)), &before).unwrap_err();
        assert!(err.contains("changed on the server meanwhile"), "{err}");
        assert!(h.join().unwrap().iter().all(|s| !s.starts_with("PUT")));
        // gone from the server: also a change for a save
        let (b, h) = mock_http(2, |_, _, _, _| (404, String::new()));
        let acct = Account { url: format!("{b}/c/"), username: String::new(), password: Default::default() };
        let err = put_event(&acct.url, &acct, std::slice::from_ref(&e), Some((&format!("{b}/c/a.ics"), None)), &before).unwrap_err();
        assert!(err.contains("changed on the server meanwhile"), "{err}");
        assert!(h.join().unwrap().iter().all(|s| !s.starts_with("PUT")));
        // a delete of a changed event sends no DELETE
        let changed = SERVER_ICS.replace("Lunch", "Brunch");
        let (b, h) = mock_http(1, move |_, _, _, _| (200, changed.clone()));
        let acct = Account { url: format!("{b}/c/"), username: String::new(), password: Default::default() };
        let err = delete_event(&format!("{b}/c/a.ics"), &acct, None, &before).unwrap_err();
        assert!(err.contains("changed on the server meanwhile"), "{err}");
        assert_eq!(h.join().unwrap(), vec!["GET /c/a.ics".to_string()]);
        // with a version tag there is no extra GET
        let (b, h) = mock_http(1, |_, _, _, _| (204, String::new()));
        let acct = Account { url: format!("{b}/c/"), username: String::new(), password: Default::default() };
        delete_event(&format!("{b}/c/a.ics"), &acct, Some("\"v1\""), &before).unwrap();
        assert_eq!(h.join().unwrap(), vec!["DELETE /c/a.ics".to_string()]);
    }

    // spec: CAL-004
    #[test]
    fn headers_come_from_the_last_answer() {
        assert_eq!(last_header("HTTP/1.1 301 Moved\r\nETag: \"old\"\r\nLocation: /b\r\n\r\nHTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 201 Created\r\netag: \"new\"\r\n\r\n", "etag").as_deref(), Some("\"new\""));
        assert_eq!(last_header("HTTP/2 204\r\n\r\n", "etag"), None);
        if !has_curl() {
            return;
        }
        let (base, _h) = mock_http_with(2, |_, path, _, _| match path {
            "/a" => (301, "Location: /b\r\nETag: \"old\"\r\n".into(), String::new()),
            _ => (207, "ETag: \"new\"\r\n".into(), "<x>HTTP/1.1 200 OK\r\n\r\nnot a header</x>".into()),
        });
        let acct = Account { url: base.clone(), username: String::new(), password: Default::default() };
        let r = request("PROPFIND", &format!("{base}/a"), "0", "<x/>", &acct).unwrap();
        assert_eq!((r.status, r.etag.as_deref(), r.url.as_str()), (207, Some("\"new\""), format!("{base}/b").as_str()));
        assert_eq!(r.body, "<x>HTTP/1.1 200 OK\r\n\r\nnot a header</x>");
    }

    // spec: CAL-004, SEC-004
    #[test]
    fn fetched_events_know_their_address() {
        if !has_curl() {
            return;
        }
        let (base, _h) = mock_http(1, |_, _, _, _| {
            (
                207,
                r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/cal/rw/a.ics</d:href><d:propstat><d:prop><d:getetag>"e1"</d:getetag><c:calendar-data>BEGIN:VCALENDAR
BEGIN:VEVENT
UID:a
DTSTART:20261113T090000Z
DTEND:20261113T093000Z
SUMMARY:A
END:VEVENT
END:VCALENDAR
</c:calendar-data></d:prop></d:propstat></d:response><d:response><d:href>https://evil.example/b.ics</d:href><d:propstat><d:prop><d:getetag>"x"&#13;&#10;X-Evil: 1</d:getetag><c:calendar-data><![CDATA[BEGIN:VCALENDAR
BEGIN:VEVENT
UID:b
DTSTART:20261113T100000Z
DTEND:20261113T103000Z
SUMMARY:B <elsewhere>
END:VEVENT
END:VCALENDAR
]]></c:calendar-data></d:prop></d:propstat></d:response></d:multistatus>"#
                    .to_string(),
            )
        });
        let acct = Account { url: base.clone(), username: String::new(), password: Default::default() };
        let ev = fetch_events(&format!("{base}/cal/rw/"), &acct, 0, i64::MAX / 4).unwrap();
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].href.as_deref(), Some(format!("{base}/cal/rw/a.ics").as_str()));
        assert_eq!(ev[0].etag.as_deref(), Some("\"e1\""));
        assert_eq!(ev[1].summary, "B <elsewhere>");
        assert_eq!((ev[1].href.as_deref(), ev[1].etag.as_deref()), (None, None), "an address on another server is not kept");
    }

    // spec: CAL-002
    #[test]
    fn calendar_state_reads_tag_and_permission() {
        if !has_curl() {
            return;
        }
        let (base, _h) = mock_http(1, |_, _, _, _| {
            (207, r#"<d:multistatus xmlns:d="DAV:" xmlns:cs="http://calendarserver.org/ns/"><d:response><d:href>/c/</d:href><d:propstat><d:prop><cs:getctag>42</cs:getctag><d:current-user-privilege-set><d:privilege><d:read/></d:privilege></d:current-user-privilege-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat><d:propstat><d:prop><d:sync-token/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat></d:response></d:multistatus>"#.into())
        });
        let acct = Account { url: format!("{base}/c/"), username: String::new(), password: Default::default() };
        let st = calendar_state(&acct.url, &acct).unwrap();
        assert_eq!(st, CalState { tag: Some("ctag:42".into()), read_only: Some(true) });
    }

    // spec: CAL-010
    #[test]
    fn unreachable_server_is_an_error() {
        if !has_curl() {
            return;
        }
        let acct = Account { url: "http://127.0.0.1:1/".into(), username: String::new(), password: Default::default() };
        assert!(discover(&acct).is_err());
    }
}
