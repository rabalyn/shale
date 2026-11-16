//! The CalDAV client against a scripted HTTP server on a loopback port (`cal_dav::tests::mock_http_with`):
//! redirects and error statuses of discovery, saving and deleting, and answers that are not HTTP
//! at all. No container is needed; the real-server suite is `caldav_env_tests.rs`.

use crate::cal_dav::tests::mock_http_with;
use crate::cal_dav::{self, Account};
use crate::cal_ics::{self, Event};
use std::io::{Read, Write};
use std::net::TcpListener;

fn has_curl() -> bool {
    std::process::Command::new("curl").arg("--version").output().is_ok()
}

fn acct(url: &str) -> Account {
    Account { url: url.into(), username: "ada".into(), password: "pw".into() }
}

const EMPTY: &str = r#"<d:multistatus xmlns:d="DAV:"/>"#;

/// An answer that lists one calendar at `/dav/work/`.
fn calendar_answer() -> String {
    r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/dav/work/</d:href><d:propstat><d:prop><d:displayname>Work</d:displayname><d:resourcetype><d:collection/><c:calendar/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#.into()
}

fn redirect(status: u16, to: &str) -> (u16, String, String) {
    (status, format!("Location: {to}\r\n"), String::new())
}

fn event() -> Event {
    Event { uid: "ev1".into(), summary: "Lunch".into(), start: 1_800_000_000, end: 1_800_001_800, ..Default::default() }
}

/// A server that answers every request (`n` of them) with this status and body.
fn answering(n: usize, status: u16, body: &'static str) -> (String, std::thread::JoinHandle<Vec<String>>) {
    mock_http_with(n, move |_, _, _, _| (status, String::new(), body.into()))
}

/// A server that sends `bytes` to the first connection, whatever it asks, and hangs up.
fn raw_server(bytes: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    let h = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = s.read(&mut buf);
        s.write_all(&bytes).unwrap();
    });
    (base, h)
}

// spec: CAL-019
#[test]
fn discovery_says_in_words_what_the_server_answered() {
    if !has_curl() {
        return;
    }
    // (status of the answer, its body, requests it takes, words in the error)
    for (status, body, requests, words) in [
        (401, "", 1, "refused the user name or password"),
        (403, "", 1, "refused the user name or password"),
        (404, "", 2, "nothing found at this address (404)"),
        (500, "", 1, "status 500"),
        (503, "busy", 1, "status 503"),
        (200, "this is not xml at all", 2, "could not be read"),
        (200, "<html><body>Welcome", 2, "could not be read"),
        (207, EMPTY, 3, "no calendars with events found"),
    ] {
        let (base, h) = answering(requests, status, body);
        let err = cal_dav::discover(&acct(&format!("{base}/"))).unwrap_err();
        assert!(err.contains(words), "{status}: {err}");
        let seen = h.join().unwrap();
        assert_eq!(seen.len(), requests, "{status}: {seen:?}");
        if requests > 1 {
            assert_eq!(seen[1], "PROPFIND /.well-known/caldav", "{status}: the well-known address is tried second");
        }
    }
}

// spec: CAL-019
#[test]
fn discovery_asks_the_well_known_address_when_the_first_is_not_found() {
    if !has_curl() {
        return;
    }
    for first in [404, 405] {
        let (base, h) = mock_http_with(2, move |_, path, _, _| if path == "/.well-known/caldav" { (207, String::new(), calendar_answer()) } else { (first, String::new(), String::new()) });
        let found = cal_dav::discover(&acct(&format!("{base}/"))).unwrap();
        assert_eq!(found.len(), 1, "{first}");
        assert_eq!(found[0].url, format!("{base}/dav/work/"));
        assert_eq!(h.join().unwrap(), vec!["PROPFIND /".to_string(), "PROPFIND /.well-known/caldav".into()]);
    }
}

// spec: CAL-019
#[test]
fn discovery_follows_each_kind_of_redirect_on_the_same_server() {
    if !has_curl() {
        return;
    }
    for status in [301, 302, 307, 308] {
        // the entered address is sent on
        let (base, h) = mock_http_with(2, move |_, path, _, _| if path == "/" { redirect(status, "/dav/") } else { (207, String::new(), calendar_answer()) });
        let found = cal_dav::discover(&acct(&format!("{base}/"))).unwrap();
        assert_eq!(found[0].url, format!("{base}/dav/work/"), "{status}");
        assert_eq!(h.join().unwrap(), vec!["PROPFIND /".to_string(), "PROPFIND /dav/".into()], "{status}: the method stays PROPFIND");
        // the well-known address is sent on (the usual way hosted services do it)
        let (base, h) = mock_http_with(3, move |_, path, _, _| match path {
            "/" => (404, String::new(), String::new()),
            "/.well-known/caldav" => redirect(status, "/dav/"),
            _ => (207, String::new(), calendar_answer()),
        });
        let found = cal_dav::discover(&acct(&format!("{base}/"))).unwrap();
        assert_eq!(found[0].url, format!("{base}/dav/work/"), "{status}");
        assert_eq!(h.join().unwrap().last().unwrap(), "PROPFIND /dav/", "{status}");
    }
}

// spec: CAL-019, SEC-001
#[test]
fn a_redirect_to_another_server_never_gets_the_password() {
    if !has_curl() {
        return;
    }
    for status in [301, 302, 307, 308] {
        let auth_seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<bool>::new()));
        let a2 = auth_seen.clone();
        // "another server" is the same computer on another port
        let (other, ho) = mock_http_with(1, move |_, _, headers, _| {
            a2.lock().unwrap().push(headers.to_ascii_lowercase().contains("authorization:"));
            (401, "WWW-Authenticate: Basic realm=\"x\"\r\n".into(), String::new())
        });
        let target = format!("{other}/dav/");
        let (base, h) = mock_http_with(1, move |_, _, _, _| redirect(status, &target));
        let err = cal_dav::discover(&acct(&format!("{base}/"))).unwrap_err();
        assert!(err.contains("does not send your password to another server"), "{status}: {err}");
        assert_eq!(h.join().unwrap().len(), 1);
        assert_eq!(ho.join().unwrap().len(), 1, "no second request: http is not retried");
        assert_eq!(*auth_seen.lock().unwrap(), vec![false], "{status}: curl keeps the password away from the other server");
    }
}

// spec: CAL-019, SEC-001
#[test]
fn redirects_that_are_not_allowed_are_refused() {
    if !has_curl() {
        return;
    }
    // to a scheme that is neither http nor https: nothing is requested there
    let (base, h) = mock_http_with(1, |_, _, _, _| redirect(301, "ftp://127.0.0.1/x"));
    let err = cal_dav::request_with("GET", &format!("{base}/x"), &[], "", &acct(&base)).unwrap_err();
    assert!(err.contains("kind of address that is not allowed"), "{err}");
    h.join().unwrap();
    // a redirect that never ends stops after 5 follow-ups and is an error in words (CAL-030)
    let (base, h) = mock_http_with(6, |_, path, _, _| redirect(302, &format!("{path}x")));
    let err = cal_dav::request_with("GET", &format!("{base}/a"), &[], "", &acct(&base)).unwrap_err();
    assert!(err.contains("redirect loop"), "{err}");
    assert_eq!(h.join().unwrap().len(), 6);
}

// spec: CAL-019, SEC-001
#[test]
fn a_redirect_is_not_followed_when_the_caller_says_so_and_an_access_token_stays_home() {
    if !has_curl() {
        return;
    }
    let (base, h) = mock_http_with(1, |_, _, _, _| redirect(307, "/elsewhere"));
    let r = cal_dav::request_opts("POST", &format!("{base}/token"), &[], "a=b", &acct(&base), false).unwrap();
    assert_eq!((r.status, r.url), (307, format!("{base}/token")), "the answer is the redirect itself");
    assert_eq!(h.join().unwrap(), vec!["POST /token".to_string()]);
    // a bearer token is never sent on to another server, even if curl would
    let (other, ho) = answering(1, 200, "");
    let target = format!("{other}/x");
    let (base, h) = mock_http_with(1, move |_, _, _, _| redirect(302, &target));
    let err = cal_dav::request_with("GET", &format!("{base}/x"), &["Authorization: Bearer secret"], "", &acct("")).unwrap_err();
    assert!(err.contains("does not send your access token to another server"), "{err}");
    h.join().unwrap();
    ho.join().unwrap();
}

// spec: CAL-020
#[test]
fn saving_names_what_the_server_refused() {
    if !has_curl() {
        return;
    }
    let e = event();
    // (status, creating, words)
    for (status, creating, words) in [
        (401, true, "refused the user name or password"),
        (403, true, "permission to write"),
        (405, true, "read-only"),
        (501, true, "read-only"),
        (412, true, "same ID already exists"),
        (412, false, "changed on the server meanwhile"),
        (415, true, "doesn't accept events"),
        (507, true, "no space left"),
        (500, true, "status 500"),
        (404, false, "status 404"),
        (502, false, "status 502"),
    ] {
        let (base, h) = answering(1, status, "garbage <<<");
        let cal = format!("{base}/cal/");
        let existing = format!("{base}/cal/ev1.ics");
        let r = cal_dav::put_event(&cal, &acct(&cal), std::slice::from_ref(&e), if creating { None } else { Some((existing.as_str(), Some("\"v1\""))) }, &[]);
        let err = r.unwrap_err();
        assert!(err.contains(words), "{status}: {err}");
        assert_eq!(h.join().unwrap().len(), 1, "{status}: one request, no retry");
    }
}

// spec: CAL-020
#[test]
fn a_save_that_is_sent_on_keeps_its_body() {
    if !has_curl() {
        return;
    }
    for status in [307, 308] {
        let (base, h) =
            mock_http_with(2, move |_, path, _, body| if path.starts_with("/old/") { redirect(status, "/new/ev1.ics") } else { (if body.contains("SUMMARY:Lunch") { 201 } else { 400 }, "ETag: \"n1\"\r\n".into(), String::new()) });
        let cal = format!("{base}/old/");
        let (href, etag) = cal_dav::put_event(&cal, &acct(&cal), &[event()], None, &[]).unwrap();
        assert!(href.starts_with(&format!("{base}/old/")), "the event's address is the one Shale chose: {href}");
        assert_eq!(etag.as_deref(), Some("\"n1\""));
        let seen = h.join().unwrap();
        assert!(seen[1].starts_with("PUT /new/"), "{status}: the body went along to the new place: {seen:?}");
    }
}

// spec: CAL-020
#[test]
fn deleting_names_what_the_server_refused() {
    if !has_curl() {
        return;
    }
    // with a version tag the DELETE is the only request
    for (status, words) in
        [(200, ""), (204, ""), (404, ""), (410, ""), (401, "refused the user name or password"), (403, "permission to write"), (405, "read-only"), (412, "changed on the server meanwhile"), (500, "status 500"), (507, "no space left")]
    {
        let (base, h) = answering(1, status, "garbage");
        let a = acct(&format!("{base}/cal/"));
        let r = cal_dav::delete_event(&format!("{base}/cal/ev1.ics"), &a, Some("\"v1\""), &[]);
        if words.is_empty() {
            r.unwrap_or_else(|e| panic!("{status}: {e}"));
        } else {
            let err = r.unwrap_err();
            assert!(err.contains(words), "{status}: {err}");
        }
        assert_eq!(h.join().unwrap().len(), 1, "{status}");
    }
}

// spec: CAL-020, CAL-016
#[test]
fn deleting_without_a_version_tag_reads_first_and_names_what_went_wrong() {
    if !has_curl() {
        return;
    }
    const ICS: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:ev1\r\nDTSTART:20261113T090000Z\r\nSUMMARY:Lunch\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let before = cal_ics::parse_ics(ICS);
    // the look at the event fails: no DELETE follows
    for (status, words) in [(401, "refused the user name or password"), (403, "could not check the event on the server first"), (500, "status 500")] {
        let (base, h) = answering(1, status, "");
        let err = cal_dav::delete_event(&format!("{base}/cal/ev1.ics"), &acct(&format!("{base}/cal/")), None, &before).unwrap_err();
        assert!(err.contains(words), "{status}: {err}");
        assert_eq!(h.join().unwrap(), vec!["GET /cal/ev1.ics".to_string()]);
    }
    // gone already: nothing to delete
    for status in [404, 410] {
        let (base, h) = answering(1, status, "");
        cal_dav::delete_event(&format!("{base}/cal/ev1.ics"), &acct(&format!("{base}/cal/")), None, &before).unwrap();
        assert_eq!(h.join().unwrap().len(), 1, "{status}");
    }
    // it reads, is the same, and the DELETE is refused
    for (status, words) in [(412, "changed on the server meanwhile"), (403, "permission to write"), (500, "status 500")] {
        let (base, h) = mock_http_with(2, move |m, _, _, _| if m == "GET" { (200, String::new(), ICS.into()) } else { (status, String::new(), String::new()) });
        let err = cal_dav::delete_event(&format!("{base}/cal/ev1.ics"), &acct(&format!("{base}/cal/")), None, &before).unwrap_err();
        assert!(err.contains(words), "{status}: {err}");
        assert_eq!(h.join().unwrap(), vec!["GET /cal/ev1.ics".to_string(), "DELETE /cal/ev1.ics".into()]);
    }
    // what it reads is rubbish: not the event Shale read, so nothing is deleted
    let (base, h) = answering(1, 200, "<<< not a calendar >>>");
    let err = cal_dav::delete_event(&format!("{base}/cal/ev1.ics"), &acct(&format!("{base}/cal/")), None, &before).unwrap_err();
    assert!(err.contains("changed on the server meanwhile"), "{err}");
    assert_eq!(h.join().unwrap().len(), 1);
}

// spec: CAL-020, SEC-001
#[test]
fn a_delete_sent_to_another_server_by_a_redirect_asks_for_the_right_address() {
    if !has_curl() {
        return;
    }
    let (other, ho) = answering(1, 401, "");
    let target = format!("{other}/cal/ev1.ics");
    let (base, h) = mock_http_with(1, move |_, _, _, _| redirect(301, &target));
    let err = cal_dav::delete_event(&format!("{base}/cal/ev1.ics"), &acct(&format!("{base}/cal/")), Some("\"v1\""), &[]).unwrap_err();
    assert!(err.contains("does not send your password to another server"), "{err}");
    h.join().unwrap();
    ho.join().unwrap();
    // on the same server a redirect is followed with the method kept
    let (base, h) = mock_http_with(2, |_, path, _, _| if path == "/cal/ev1.ics" { redirect(308, "/moved/ev1.ics") } else { (204, String::new(), String::new()) });
    cal_dav::delete_event(&format!("{base}/cal/ev1.ics"), &acct(&format!("{base}/cal/")), Some("\"v1\""), &[]).unwrap();
    assert_eq!(h.join().unwrap(), vec!["DELETE /cal/ev1.ics".to_string(), "DELETE /moved/ev1.ics".into()]);
}

// spec: CAL-019
#[test]
fn an_answer_that_is_not_http_is_an_error() {
    if !has_curl() {
        return;
    }
    for bytes in [&b"this is not http\r\n"[..], &b""[..], &b"HTTP/1.1 2"[..]] {
        let (base, h) = raw_server(bytes.to_vec());
        let err = cal_dav::discover(&acct(&format!("{base}/"))).unwrap_err();
        assert!(!err.is_empty(), "{bytes:?}");
        h.join().unwrap();
    }
    // nobody listens at all
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let err = cal_dav::discover(&acct(&format!("http://127.0.0.1:{port}/"))).unwrap_err();
    assert!(!err.is_empty());
}

// spec: SEC-001
#[test]
fn bad_requests_are_refused_before_anything_is_sent() {
    let a = acct("http://127.0.0.1:1/");
    let url = "http://127.0.0.1:1/";
    assert!(cal_dav::request_bytes("get", url, &[], "", &a, 10).unwrap_err().contains("bad request method"));
    assert!(cal_dav::request_bytes("", url, &[], "", &a, 10).unwrap_err().contains("bad request method"));
    let colon = Account { username: "a:b".into(), ..a.clone() };
    assert!(cal_dav::request_bytes("GET", url, &[], "", &colon, 10).unwrap_err().contains("user name"));
    let nul = Account { password: "a\0b".into(), ..a.clone() };
    assert!(cal_dav::request_bytes("GET", url, &[], "", &nul, 10).unwrap_err().contains("NUL"));
    assert!(cal_dav::request_bytes("PUT", url, &[], "a\0b", &a, 10).unwrap_err().contains("NUL"));
}

// spec: SEC-001
#[test]
fn an_answer_bigger_than_allowed_is_an_error_and_a_small_one_keeps_its_headers() {
    if !has_curl() {
        return;
    }
    let (base, h) = mock_http_with(2, |_, _, _, _| (200, "ETag: \"abc\"\r\n".into(), "x".repeat(5000)));
    let err = cal_dav::request_bytes("GET", &format!("{base}/big"), &[], "", &acct(&base), 1000);
    assert!(err.unwrap_err().contains("bigger than 1000 bytes"));
    let ok = cal_dav::request_bytes("GET", &format!("{base}/big"), &[], "", &acct(&base), 10_000).unwrap();
    assert_eq!((ok.status, ok.body.len(), ok.etag.as_deref(), ok.content_type.as_deref()), (200, 5000, Some("\"abc\""), Some("application/xml")));
    h.join().unwrap();
}

// spec: CAL-030
#[test]
fn a_redirect_loop_is_named_and_not_shown_as_its_last_status() {
    if !has_curl() {
        return;
    }
    // curl follows five redirects and stops at the sixth answer
    let (base, h) = mock_http_with(6, |_, _, _, _| redirect(302, "/again"));
    let url = format!("{base}/loop");
    let err = cal_dav::request_bytes("GET", &url, &[], "", &acct(&base), 10_000).unwrap_err();
    assert!(err.contains("redirect loop"), "{err}");
    h.join().unwrap();
    let (base, h) = mock_http_with(6, |_, _, _, _| redirect(302, "/again"));
    let err = cal_dav::discover(&acct(&format!("{base}/"))).unwrap_err();
    assert!(err.contains("redirect loop") && !err.contains("302"), "{err}");
    h.join().unwrap();
}

// spec: CAL-030
#[test]
fn an_empty_reply_a_refused_connection_and_a_cut_short_answer_are_named() {
    if !has_curl() {
        return;
    }
    let (base, h) = raw_server(Vec::new());
    let err = cal_dav::request_bytes("GET", &format!("{base}/"), &[], "", &acct(&base), 10_000).unwrap_err();
    assert!(err.contains("closed the connection without an answer"), "{err}");
    h.join().unwrap();

    // nobody listens at this port
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let base = format!("http://127.0.0.1:{port}");
    let err = cal_dav::request_bytes("GET", &format!("{base}/"), &[], "", &acct(&base), 10_000).unwrap_err();
    assert!(err.contains("could not connect"), "{err}");

    // a 200 with a body that stops before its announced length is not a success
    let (base, h) = raw_server(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nabc".to_vec());
    let err = cal_dav::request_bytes("GET", &format!("{base}/"), &[], "", &acct(&base), 10_000).unwrap_err();
    assert!(err.contains("cut short"), "{err}");
    h.join().unwrap();
}
