//! A pretend Mastodon and Bluesky server for the sign-in tests: plain HTTP on 127.0.0.1 (the
//! only place where Shale accepts `http://`), and a pretend browser for the Mastodon sign-in
//! page. The server checks what a real one checks: the app registration, the client ID and
//! secret, a code that works once, the bearer token, the app password.

use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
pub struct ServerState {
    pub counter: u32,
    /// (client ID, secret) of the registered apps
    pub apps: Vec<(String, String)>,
    pub codes: HashSet<String>,
    pub tokens: HashSet<String>,
    /// `METHOD path` of every request, in order.
    pub requests: Vec<String>,
    /// The only Bluesky app password that works.
    pub app_password: String,
}

pub struct FakeServer {
    pub port: u16,
    pub state: Arc<Mutex<ServerState>>,
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl FakeServer {
    pub fn start() -> FakeServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(ServerState { app_password: "abcd-efgh-ijkl-mnop".into(), ..ServerState::default() }));
        let stop = Arc::new(AtomicBool::new(false));
        let (st, sp) = (state.clone(), stop.clone());
        let join = std::thread::spawn(move || {
            while !sp.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((s, _)) => {
                        let st = st.clone();
                        std::thread::spawn(move || {
                            let _ = s.set_nonblocking(false);
                            serve(s, &st);
                        });
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(3)),
                }
            }
        });
        FakeServer { port, state, stop, join: Some(join) }
    }

    pub fn address(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn query_value(query: &str, key: &str) -> String {
    query.split('&').filter_map(|p| p.split_once('=')).find(|(k, _)| *k == key).map(|(_, v)| v.replace("%3A", ":").replace("%2F", "/")).unwrap_or_default()
}

fn respond(s: &mut TcpStream, status: u16, ctype: &str, body: &str) {
    let _ = write!(s, "HTTP/1.1 {status} X\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
}

fn serve(mut s: TcpStream, st: &Arc<Mutex<ServerState>>) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    let end = loop {
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p;
        }
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    let len: usize = head.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.trim().parse().ok())).unwrap_or(0);
    while buf.len() < end + 4 + len {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let body: Value = serde_json::from_slice(&buf[(end + 4).min(buf.len())..]).unwrap_or(Value::Null);
    let bearer = head.lines().find_map(|l| l.strip_prefix("Authorization: Bearer ").or_else(|| l.strip_prefix("authorization: Bearer "))).unwrap_or("").trim().to_string();
    let mut first = head.lines().next().unwrap_or("").split(' ');
    let (method, target) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut g = st.lock().unwrap();
    g.requests.push(format!("{method} {path}"));
    let field = |k: &str| body[k].as_str().unwrap_or("").to_string();
    match (method, path) {
        ("POST", "/api/v1/apps") => {
            if field("redirect_uris") != "urn:ietf:wg:oauth:2.0:oob" || field("scopes") != "read" {
                return respond(&mut s, 422, "application/json", r#"{"error":"Validation failed"}"#);
            }
            g.counter += 1;
            let app = (format!("cid-{}", g.counter), format!("secret-{}", g.counter));
            g.apps.push(app.clone());
            respond(&mut s, 200, "application/json", &json!({"client_id": app.0, "client_secret": app.1}).to_string());
        }
        // the page where the person logs in, allows the app and is shown the code
        ("GET", "/oauth/authorize") => {
            let known = g.apps.iter().any(|a| a.0 == query_value(query, "client_id"));
            if !known || query_value(query, "response_type") != "code" || query_value(query, "scope") != "read" || query_value(query, "redirect_uri") != "urn:ietf:wg:oauth:2.0:oob" {
                return respond(&mut s, 400, "text/html", "<p>Error</p>");
            }
            g.counter += 1;
            let code = format!("code-{}", g.counter);
            g.codes.insert(code.clone());
            respond(&mut s, 200, "text/html", &format!("<p>Authorization code</p><code>{code}</code>"));
        }
        ("POST", "/oauth/token") => {
            let app_ok = g.apps.iter().any(|a| a.0 == field("client_id") && a.1 == field("client_secret"));
            if !app_ok {
                return respond(&mut s, 401, "application/json", r#"{"error":"invalid_client","error_description":"Client authentication failed"}"#);
            }
            if field("grant_type") != "authorization_code" || field("redirect_uri") != "urn:ietf:wg:oauth:2.0:oob" || !g.codes.remove(&field("code")) {
                return respond(&mut s, 400, "application/json", r#"{"error":"invalid_grant","error_description":"The provided authorization grant is invalid, expired, revoked"}"#);
            }
            g.counter += 1;
            let token = format!("tok-{}", g.counter);
            g.tokens.insert(token.clone());
            respond(&mut s, 200, "application/json", &json!({"access_token": token, "token_type": "Bearer", "scope": "read"}).to_string());
        }
        ("GET", "/api/v1/accounts/verify_credentials") => {
            if g.tokens.contains(&bearer) {
                respond(&mut s, 200, "application/json", &json!({"acct": "ada", "display_name": "Ada L."}).to_string());
            } else {
                respond(&mut s, 401, "application/json", r#"{"error":"The access token is invalid"}"#);
            }
        }
        ("POST", "/xrpc/com.atproto.server.createSession") => {
            if field("password") == g.app_password && field("identifier") == "ada.test" {
                respond(&mut s, 200, "application/json", &json!({"accessJwt": "access", "refreshJwt": "refresh", "did": "did:plc:ada", "handle": "ada.test"}).to_string());
            } else {
                respond(&mut s, 401, "application/json", r#"{"error":"AuthenticationRequired","message":"Invalid identifier or password"}"#);
            }
        }
        _ => respond(&mut s, 404, "application/json", "{}"),
    }
}

/// What the person does on the Mastodon sign-in page.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Person {
    /// Logs in, allows the app and reads the code off the page.
    Allows,
    /// Says no: there is no code.
    Denies,
}

/// The record of the pretend browser.
#[derive(Clone, Default)]
pub struct BrowserLog {
    pub opened: Arc<Mutex<Vec<String>>>,
    /// The code the sign-in page showed.
    pub code: Arc<Mutex<Option<String>>>,
    /// What was wrong with the address, as sentences.
    pub problems: Arc<Mutex<Vec<String>>>,
}

impl BrowserLog {
    /// The code the person read off the page (waits for the browser).
    pub fn wait_for_code(&self) -> String {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(c) = self.code.lock().unwrap().clone() {
                return c;
            }
            assert!(std::time::Instant::now() < deadline, "the person never got a code");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// The pretend browser for a pretend server on `port`: checks the sign-in address (the app
/// registered there, read-only scope, the out-of-band redirect, a code answer) and acts as the person.
pub fn pretend_browser(port: u16, person: Person) -> (crate::opener::BrowserGuard, BrowserLog) {
    let log = BrowserLog::default();
    let l = log.clone();
    let guard = crate::opener::install_test_browser_for_local_server(
        port,
        Box::new(move |url| {
            l.opened.lock().unwrap().push(url.to_string());
            let (url, l) = (url.to_string(), l.clone());
            std::thread::spawn(move || {
                if !url.starts_with(&format!("http://127.0.0.1:{port}/oauth/authorize?")) {
                    l.problems.lock().unwrap().push(format!("not the server's sign-in page: {url}"));
                }
                if person == Person::Allows {
                    let (status, _, body) = crate::mail::oauth_fake_tests::http_get(&url);
                    match body.split_once("<code>").and_then(|(_, r)| r.split_once("</code>")) {
                        Some((code, _)) if status == 200 => *l.code.lock().unwrap() = Some(code.to_string()),
                        _ => l.problems.lock().unwrap().push(format!("the sign-in page answered {status}: {body}")),
                    }
                }
            });
        }),
    );
    (guard, log)
}
