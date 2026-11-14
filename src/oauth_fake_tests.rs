//! A pretend OAuth2 provider for tests: plain HTTP on 127.0.0.1 (the Custom provider may use
//! that), with `/authorize` (answers with a redirect carrying a code and the `state`) and
//! `/token` (authorization code with PKCE, and refresh tokens). It checks what a real provider
//! checks: client ID, redirect address, PKCE verifier, a code that works once, a revoked token.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct FakeOAuthState {
    pub client_id: String,
    pub client_secret: Option<String>,
    /// code -> (PKCE challenge, redirect address)
    pub codes: HashMap<String, (String, String)>,
    pub refresh_tokens: HashSet<String>,
    /// The access tokens that are good: share this with the pretend IMAP server.
    pub valid_access: Arc<Mutex<HashSet<String>>>,
    pub expires_in: u64,
    pub counter: u32,
    /// The forms received at `/token`.
    pub token_requests: Vec<Vec<(String, String)>>,
    /// The query received at `/authorize`.
    pub authorize_requests: Vec<Vec<(String, String)>>,
    /// Refresh tokens are refused with `invalid_grant`.
    pub revoked: bool,
    /// Every refresh hands out a new refresh token (and the old one dies).
    pub rotate: bool,
    /// `/authorize` answers `error=access_denied`.
    pub deny: bool,
    pub email: Option<String>,
    pub redirect_token_to: Option<String>,
}

pub struct FakeOAuth {
    pub port: u16,
    pub state: Arc<Mutex<FakeOAuthState>>,
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let h = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16).unwrap_or(b'?');
                out.push(h);
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn pairs(q: &str) -> Vec<(String, String)> {
    q.split('&').filter(|p| !p.is_empty()).map(|p| p.split_once('=').map(|(k, v)| (pct_decode(k), pct_decode(v))).unwrap_or((pct_decode(p), String::new()))).collect()
}

fn get(v: &[(String, String)], k: &str) -> String {
    v.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default()
}

/// A plain HTTP GET that does not follow redirects: (status, headers, body). For tests that
/// play the browser.
pub fn http_get(url: &str) -> (u16, String, String) {
    let rest = url.strip_prefix("http://").expect("an http address");
    let (authority, path) = rest.split_once('/').map(|(a, p)| (a, format!("/{p}"))).unwrap_or((rest, "/".into()));
    let mut s = TcpStream::connect(authority).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n").unwrap();
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, head.to_string(), body.to_string())
}

/// A plain HTTP POST of a form, without following redirects: (status, headers, body).
pub fn http_post_form(url: &str, form: &str) -> (u16, String, String) {
    let rest = url.strip_prefix("http://").expect("an http address");
    let (authority, path) = rest.split_once('/').map(|(a, p)| (a, format!("/{p}"))).unwrap_or((rest, "/".into()));
    let mut s = TcpStream::connect(authority).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    write!(s, "POST {path} HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{form}", form.len()).unwrap();
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, head.to_string(), body.to_string())
}

impl FakeOAuth {
    pub fn start() -> FakeOAuth {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(FakeOAuthState {
            client_id: "shale-test".into(),
            client_secret: None,
            codes: HashMap::new(),
            refresh_tokens: HashSet::new(),
            valid_access: Arc::new(Mutex::new(HashSet::new())),
            expires_in: 3600,
            counter: 0,
            token_requests: vec![],
            authorize_requests: vec![],
            revoked: false,
            rotate: false,
            deny: false,
            email: Some("ada@example.com".into()),
            redirect_token_to: None,
        }));
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
        FakeOAuth { port, state, stop, join: Some(join) }
    }

    /// The access tokens that count as valid (give this to the pretend IMAP server).
    pub fn valid_access(&self) -> Arc<Mutex<HashSet<String>>> {
        self.state.lock().unwrap().valid_access.clone()
    }

    /// A code as `/authorize` would hand out.
    pub fn issue_code(&self, challenge: &str, redirect: &str) -> String {
        let mut g = self.state.lock().unwrap();
        g.counter += 1;
        let code = format!("code-{}", g.counter);
        g.codes.insert(code.clone(), (challenge.to_string(), redirect.to_string()));
        code
    }

    pub fn new_refresh_token(&self) -> String {
        let mut g = self.state.lock().unwrap();
        g.counter += 1;
        let t = format!("rt-{}", g.counter);
        g.refresh_tokens.insert(t.clone());
        t
    }

    /// Make every access token handed out so far invalid (the server "forgets" them).
    pub fn expire_access_tokens(&self) {
        self.valid_access().lock().unwrap().clear();
    }
}

impl Drop for FakeOAuth {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn respond(s: &mut TcpStream, status: u16, extra: &str, body: &str) {
    let _ = write!(s, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}", body.len());
}

fn serve(mut s: TcpStream, st: &Arc<Mutex<FakeOAuthState>>) {
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
    let body = String::from_utf8_lossy(&buf[(end + 4).min(buf.len())..]).into_owned();
    let mut first = head.lines().next().unwrap_or("").split(' ');
    let (method, target) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut g = st.lock().unwrap();
    match (method, path) {
        ("GET", "/authorize") => {
            let q = pairs(query);
            g.authorize_requests.push(q.clone());
            let redirect = get(&q, "redirect_uri");
            let state = get(&q, "state");
            let bad = get(&q, "client_id") != g.client_id || get(&q, "response_type") != "code" || get(&q, "code_challenge_method") != "S256" || !redirect.starts_with("http://127.0.0.1:") || state.is_empty();
            if bad {
                return respond(&mut s, 400, "", r#"{"error":"invalid_request"}"#);
            }
            let loc = if g.deny {
                format!("{redirect}?error=access_denied&state={state}")
            } else {
                g.counter += 1;
                let code = format!("code-{}", g.counter);
                g.codes.insert(code.clone(), (get(&q, "code_challenge"), redirect.clone()));
                format!("{redirect}?code={code}&state={state}")
            };
            respond(&mut s, 302, &format!("Location: {loc}\r\n"), "");
        }
        ("POST", "/token") => {
            let f = pairs(&body);
            g.token_requests.push(f.clone());
            if let Some(to) = g.redirect_token_to.clone() {
                return respond(&mut s, 307, &format!("Location: {to}\r\n"), "");
            }
            let err = |s: &mut TcpStream, e: &str| respond(s, 400, "", &format!(r#"{{"error":"{e}","error_description":"pretend provider"}}"#));
            if get(&f, "client_id") != g.client_id {
                return err(&mut s, "invalid_client");
            }
            if g.client_secret.as_deref().is_some_and(|x| x != get(&f, "client_secret")) {
                return err(&mut s, "invalid_client");
            }
            let mut new_refresh: Option<String> = None;
            match get(&f, "grant_type").as_str() {
                "authorization_code" => {
                    let Some((challenge, redirect)) = g.codes.remove(&get(&f, "code")) else { return err(&mut s, "invalid_grant") };
                    let verifier = get(&f, "code_verifier");
                    if redirect != get(&f, "redirect_uri") || crate::oauth::pkce_challenge(&verifier) != challenge {
                        return err(&mut s, "invalid_grant");
                    }
                    g.counter += 1;
                    let rt = format!("rt-{}", g.counter);
                    g.refresh_tokens.insert(rt.clone());
                    new_refresh = Some(rt);
                }
                "refresh_token" => {
                    let old = get(&f, "refresh_token");
                    if g.revoked || !g.refresh_tokens.contains(&old) {
                        return err(&mut s, "invalid_grant");
                    }
                    if g.rotate {
                        g.refresh_tokens.remove(&old);
                        g.counter += 1;
                        let rt = format!("rt-{}", g.counter);
                        g.refresh_tokens.insert(rt.clone());
                        new_refresh = Some(rt);
                    }
                }
                _ => return err(&mut s, "unsupported_grant_type"),
            }
            g.counter += 1;
            let at = format!("at-{}", g.counter);
            g.valid_access.lock().unwrap().insert(at.clone());
            let mut json = format!(r#"{{"access_token":"{at}","token_type":"Bearer","expires_in":{}"#, g.expires_in);
            if let Some(rt) = new_refresh {
                json.push_str(&format!(r#","refresh_token":"{rt}""#));
            }
            if let Some(email) = &g.email {
                let payload = crate::oauth::base64url(format!(r#"{{"email":"{email}"}}"#).as_bytes());
                json.push_str(&format!(r#","id_token":"e30.{payload}.sig""#));
            }
            json.push('}');
            respond(&mut s, 200, "", &json);
        }
        _ => respond(&mut s, 404, "", "{}"),
    }
}
