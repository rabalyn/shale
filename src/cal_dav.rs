//! A small CalDAV client. HTTP(S) goes through the system's `curl` program (TLS with the system
//! certificates); the password is passed to curl on its standard input, not on the command line.
//!
//! - `discover`: from a server/account address to the list of calendars
//!   (current-user-principal → calendar-home-set → calendar collections).
//! - `fetch_events`: a calendar-query REPORT for a time range.

use crate::cal_ics::{self, Event};
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub url: String,
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FoundCalendar {
    pub url: String,
    pub name: String,
    pub color: Option<String>,
}

fn curl_quote(s: &str) -> String {
    // curl config strings: backslash escapes, and line breaks must be written as \r / \n
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\r', "\\r").replace('\n', "\\n").replace('\t', "\\t"))
}

/// Run one HTTP request with curl. Returns (status code, body).
pub fn request(method: &str, url: &str, depth: &str, body: &str, acct: &Account) -> Result<(u16, String), String> {
    request_with(method, url, &[&format!("Depth: {depth}"), "Content-Type: application/xml; charset=utf-8"], body, acct)
}

/// One HTTP request with any headers. Returns (status code, body).
pub fn request_with(method: &str, url: &str, headers: &[&str], body: &str, acct: &Account) -> Result<(u16, String), String> {
    let mut cfg = String::new();
    if !acct.username.is_empty() {
        cfg.push_str(&format!("user = {}\n", curl_quote(&format!("{}:{}", acct.username, acct.password))));
    }
    cfg.push_str(&format!("data-binary = {}\n", curl_quote(body)));
    for h in headers {
        cfg.push_str(&format!("header = {}\n", curl_quote(h)));
    }
    let mut child = Command::new("curl")
        .args(["-sS", "-L", "--max-time", "30", "-X", method, "-w", "\n__SHALE_STATUS__%{http_code}", "-K", "-", url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run curl ({e}) — is curl installed?"))?;
    child.stdin.take().unwrap().write_all(cfg.as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let Some(pos) = text.rfind("\n__SHALE_STATUS__") else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { "no answer from the server".into() } else { err });
    };
    let status: u16 = text[pos + 17..].trim().parse().unwrap_or(0);
    if status == 0 {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { "could not reach the server".into() } else { err });
    }
    Ok((status, text[..pos].to_string()))
}

fn check(status: u16) -> Result<(), String> {
    match status {
        200..=299 => Ok(()),
        401 | 403 => Err("the server refused the user name or password".into()),
        404 => Err("nothing found at this address (404)".into()),
        s => Err(format!("the server answered with status {s}")),
    }
}

// ---------------------------------------------------------------- tiny XML helpers

pub fn unescape_xml(s: &str) -> String {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix("<![CDATA[").and_then(|x| x.strip_suffix("]]>")) {
        return inner.to_string();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        let after = &rest[p..];
        let Some(end) = after.find(';') else {
            out.push_str(after);
            rest = "";
            break;
        };
        let ent = &after[1..end];
        let ch = match ent {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => out.push(c),
            None => out.push_str(&after[..=end]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Inner text of every element with this local name (namespace prefixes ignored).
pub fn elements(xml: &str, local: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = xml[i..].find('<') {
        let start = i + p;
        let tag_end = match xml[start..].find('>') {
            Some(e) => start + e,
            None => break,
        };
        let tag = &xml[start + 1..tag_end];
        i = tag_end + 1;
        if tag.starts_with('/') || tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        let qname = tag.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        if qname.rsplit(':').next() != Some(local) {
            continue;
        }
        if tag.ends_with('/') {
            out.push(String::new());
            continue;
        }
        let close = format!("</{qname}>");
        // CDATA may contain '<', skip over it when looking for the end tag
        let mut search = i;
        let end = loop {
            let Some(c) = xml[search..].find(&close) else { break None };
            let candidate = search + c;
            let cdata_open = xml[i..candidate].matches("<![CDATA[").count();
            let cdata_close = xml[i..candidate].matches("]]>").count();
            if cdata_open == cdata_close {
                break Some(candidate);
            }
            search = candidate + close.len();
        };
        if let Some(e) = end {
            out.push(xml[i..e].to_string());
            i = e + close.len();
        }
    }
    out
}

fn first(xml: &str, local: &str) -> Option<String> {
    elements(xml, local).into_iter().next()
}

/// Make an href from the server absolute.
pub fn resolve(base: &str, href: &str) -> String {
    let href = href.trim();
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    let scheme_end = base.find("://").map(|p| p + 3).unwrap_or(0);
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

// ---------------------------------------------------------------- CalDAV

const PROP_PRINCIPAL: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:current-user-principal/><d:resourcetype/><d:displayname/><c:calendar-home-set/></d:prop></d:propfind>"#;
const PROP_HOME: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><c:calendar-home-set/></d:prop></d:propfind>"#;
const PROP_LIST: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/"><d:prop><d:resourcetype/><d:displayname/><a:calendar-color/></d:prop></d:propfind>"#;

fn is_calendar(response: &str) -> bool {
    first(response, "resourcetype").map(|rt| !elements(&rt, "calendar").is_empty()).unwrap_or(false)
}

fn calendars_in(xml: &str, base: &str) -> Vec<FoundCalendar> {
    elements(xml, "response")
        .into_iter()
        .filter(|r| is_calendar(r))
        .filter_map(|r| {
            let url = resolve(base, &unescape_xml(&first(&r, "href")?));
            let name = first(&r, "displayname").map(|n| unescape_xml(&n)).filter(|n| !n.is_empty()).unwrap_or_else(|| url.trim_end_matches('/').rsplit('/').next().unwrap_or("Calendar").to_string());
            let color = first(&r, "calendar-color").map(|c| unescape_xml(&c)).filter(|c| c.starts_with('#')).map(|c| c.chars().take(7).collect());
            Some(FoundCalendar { url, name, color })
        })
        .collect()
}

/// Find the calendars of an account. The address may be a calendar, the calendar home, the
/// principal, or just the server (then `/.well-known/caldav` is tried).
pub fn discover(acct: &Account) -> Result<Vec<FoundCalendar>, String> {
    let mut url = acct.url.trim().to_string();
    if url.is_empty() {
        return Err("enter the server address".into());
    }
    let (status, body) = request("PROPFIND", &url, "0", PROP_PRINCIPAL, acct)?;
    let (status, body) = if status == 404 || status == 405 {
        let wk = resolve(&url, "/.well-known/caldav");
        url = wk.clone();
        request("PROPFIND", &wk, "0", PROP_PRINCIPAL, acct)?
    } else {
        (status, body)
    };
    check(status)?;
    if elements(&body, "response").iter().any(|r| is_calendar(r)) {
        return Ok(calendars_in(&body, &url));
    }
    // calendar home: directly, or via the principal
    let mut home = first(&body, "calendar-home-set").and_then(|h| first(&h, "href")).map(|h| resolve(&url, &unescape_xml(&h)));
    if home.is_none() {
        if let Some(principal) = first(&body, "current-user-principal").and_then(|p| first(&p, "href")) {
            let purl = resolve(&url, &unescape_xml(&principal));
            let (s, b) = request("PROPFIND", &purl, "0", PROP_HOME, acct)?;
            check(s)?;
            home = first(&b, "calendar-home-set").and_then(|h| first(&h, "href")).map(|h| resolve(&purl, &unescape_xml(&h)));
        }
    }
    let home = home.unwrap_or(url);
    let (s, b) = request("PROPFIND", &home, "1", PROP_LIST, acct)?;
    check(s)?;
    let found = calendars_in(&b, &home);
    if found.is_empty() {
        Err("no calendars found for this account".into())
    } else {
        Ok(found)
    }
}

fn utc_stamp(t: i64) -> String {
    let (y, m, d) = crate::cal_time::civil_from_days(t.div_euclid(86_400));
    let s = t.rem_euclid(86_400);
    format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z", s / 3600, s % 3600 / 60, s % 60)
}

/// Events of one calendar between two UTC times.
pub fn fetch_events(cal_url: &str, acct: &Account, from: i64, to: i64) -> Result<Vec<Event>, String> {
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"><c:time-range start="{}" end="{}"/></c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"#,
        utc_stamp(from),
        utc_stamp(to)
    );
    let (status, xml) = request("REPORT", cal_url, "1", &body, acct)?;
    check(status)?;
    let mut out = Vec::new();
    for resp in elements(&xml, "response") {
        let href = first(&resp, "href").map(|h| resolve(cal_url, &unescape_xml(&h)));
        let etag = first(&resp, "getetag").map(|e| unescape_xml(&e)).filter(|e| !e.is_empty());
        for data in elements(&resp, "calendar-data") {
            for mut e in cal_ics::parse_ics(&unescape_xml(&data)) {
                e.href = href.clone();
                e.etag = etag.clone();
                out.push(e);
            }
        }
    }
    Ok(out)
}

/// A file name for an event on the server, from its UID.
fn event_file(uid: &str) -> String {
    let clean: String = uid.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect();
    format!("{clean}.ics")
}

fn write_error(status: u16, creating: bool) -> String {
    match status {
        401 => "the server refused the user name or password".into(),
        403 => "you don't have permission to write to this calendar".into(),
        405 | 501 => "this calendar is read-only".into(),
        412 if creating => "an event with the same ID already exists in this calendar".into(),
        412 => "the event was changed on the server meanwhile; refresh the calendar and try again".into(),
        507 => "the server has no space left".into(),
        s => format!("the server answered with status {s}"),
    }
}

/// Save an event to a calendar: `parts` is the event and, for a repeating one, its changed
/// single dates (all stored together in one file on the server). `existing`: the current
/// address and version tag when it is changed in place; None creates it. Returns the address.
pub fn put_event(cal_url: &str, acct: &Account, parts: &[Event], existing: Option<(&str, Option<&str>)>) -> Result<String, String> {
    let e = parts.first().ok_or("nothing to save")?;
    let clean: Vec<Event> = parts.iter().map(|p| Event { href: None, etag: None, ..p.clone() }).collect();
    let body = cal_ics::to_ics("", &clean);
    let creating = existing.is_none();
    let url = match existing {
        Some((href, _)) => href.to_string(),
        None => resolve(&format!("{}/", cal_url.trim_end_matches('/')), &event_file(&e.uid)),
    };
    let cond = match existing {
        None => Some("If-None-Match: *".to_string()),
        Some((_, Some(tag))) => Some(format!("If-Match: {tag}")),
        Some((_, None)) => None,
    };
    let mut headers = vec!["Content-Type: text/calendar; charset=utf-8"];
    if let Some(c) = &cond {
        headers.push(c);
    }
    let (status, _) = request_with("PUT", &url, &headers, &body, acct)?;
    match status {
        200..=299 => Ok(url),
        s => Err(write_error(s, creating)),
    }
}

/// Remove an event from the server. An event that is already gone counts as removed.
pub fn delete_event(href: &str, acct: &Account, etag: Option<&str>) -> Result<(), String> {
    let cond = etag.map(|t| format!("If-Match: {t}"));
    let headers: Vec<&str> = cond.iter().map(|s| s.as_str()).collect();
    let (status, _) = request_with("DELETE", href, &headers, "", acct)?;
    match status {
        200..=299 | 404 | 410 => Ok(()),
        s => Err(write_error(s, false)),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read};
    use std::net::TcpListener;

    #[test]
    fn xml_helpers() {
        let x = r#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/cal/work/</d:href><d:propstat><d:prop><d:displayname>Work &amp; more</d:displayname><d:resourcetype><d:collection/><cal:calendar xmlns:cal="urn:ietf:params:xml:ns:caldav"/></d:resourcetype></d:prop></d:propstat></d:response><d:response><d:href>/cal/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response></d:multistatus>"#;
        let found = calendars_in(x, "https://example.com/dav/");
        assert_eq!(found, vec![FoundCalendar { url: "https://example.com/cal/work/".into(), name: "Work & more".into(), color: None }]);
        assert_eq!(unescape_xml("<![CDATA[a<b]]>"), "a<b");
        assert_eq!(unescape_xml("&#65;&#x42;&lt;"), "AB<");
        assert_eq!(resolve("https://h.example/a/b/", "c/"), "https://h.example/a/b/c/");
        assert_eq!(resolve("https://h.example/a/b/", "/x/"), "https://h.example/x/");
        assert_eq!(resolve("https://h.example", "http://o.example/y"), "http://o.example/y");
    }

    /// A pretend HTTP server on localhost: `handler(method, path, headers, body) -> (status, body)`.
    /// Serves `n` requests. Returns the base URL and, when joined, "METHOD PATH" of every request.
    pub(crate) fn mock_http(n: usize, handler: impl Fn(&str, &str, &str, &str) -> (u16, String) + Send + 'static) -> (String, std::thread::JoinHandle<Vec<String>>) {
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
                let (status, reply) = handler(&method, &path, &headers, &String::from_utf8_lossy(&body));
                let resp = format!("HTTP/1.1 {status} X\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
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

    #[test]
    fn discovers_and_fetches_from_a_server() {
        if Command::new("curl").arg("--version").output().is_err() {
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

    #[test]
    fn writes_and_removes_events() {
        if Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let (base, h) = mock_http(5, |method, path, headers, body| {
            let lower = headers.to_ascii_lowercase();
            match (method, path) {
                ("PUT", "/cal/rw/ev_1.ics") if lower.contains("if-none-match: *") && body.contains("SUMMARY:Lunch") => (201, String::new()),
                ("PUT", "/cal/rw/ev_1.ics") if lower.contains("if-match: \"v1\"") => (412, String::new()),
                ("PUT", "/cal/ro/ev_1.ics") => (403, String::new()),
                ("DELETE", "/cal/rw/ev_1.ics") => (204, String::new()),
                ("DELETE", "/cal/rw/gone.ics") => (404, String::new()),
                _ => (500, String::new()),
            }
        });
        let acct = Account { url: base.clone(), username: "ada".into(), password: "x".into() };
        let e = Event { uid: "ev/1".into(), summary: "Lunch".into(), start: 1_800_000_000, end: 1_800_001_800, ..Default::default() };
        let href = put_event(&format!("{base}/cal/rw"), &acct, std::slice::from_ref(&e), None).unwrap();
        assert_eq!(href, format!("{base}/cal/rw/ev_1.ics"));
        let err = put_event(&format!("{base}/cal/ro/"), &acct, std::slice::from_ref(&e), None).unwrap_err();
        assert!(err.contains("permission"), "{err}");
        let err = put_event("", &acct, std::slice::from_ref(&e), Some((&href, Some("\"v1\"")))).unwrap_err();
        assert!(err.contains("changed on the server"), "{err}");
        delete_event(&href, &acct, None).unwrap();
        delete_event(&format!("{base}/cal/rw/gone.ics"), &acct, None).unwrap();
        assert_eq!(h.join().unwrap().len(), 5);
    }

    #[test]
    fn fetched_events_know_their_address() {
        if Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let (base, _h) = mock_http(1, |_, _, _, _| {
            (207, r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/cal/rw/a.ics</d:href><d:propstat><d:prop><d:getetag>"e1"</d:getetag><c:calendar-data>BEGIN:VCALENDAR
BEGIN:VEVENT
UID:a
DTSTART:20261113T090000Z
DTEND:20261113T093000Z
SUMMARY:A
END:VEVENT
END:VCALENDAR
</c:calendar-data></d:prop></d:propstat></d:response></d:multistatus>"#.to_string())
        });
        let acct = Account { url: base.clone(), username: String::new(), password: String::new() };
        let ev = fetch_events(&format!("{base}/cal/rw/"), &acct, 0, i64::MAX / 4).unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].href.as_deref(), Some(format!("{base}/cal/rw/a.ics").as_str()));
        assert_eq!(ev[0].etag.as_deref(), Some("\"e1\""));
    }

    #[test]
    fn unreachable_server_is_an_error() {
        if Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let acct = Account { url: "http://127.0.0.1:1/".into(), username: String::new(), password: String::new() };
        assert!(discover(&acct).is_err());
    }
}
