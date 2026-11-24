//! OAuth2 sign-in for IMAP (MAIL-080…MAIL-085).
//!
//! The authorization-code flow with PKCE (RFC 7636, `S256`) and a `state` check:
//!
//! 1. [`begin`] binds a listener on `127.0.0.1:<random port>` and builds the sign-in address;
//! 2. the address is opened in the system browser, only through `opener::open_signin_url`;
//! 3. [`Pending::wait_for_code`] accepts exactly one valid callback (path `/`, the right
//!    `state`, the right `Host`), answers everything else with an error and goes on listening,
//!    and gives up after a timeout or when cancelled;
//! 4. [`exchange_code`] / [`refresh`] talk to the token endpoint with the system `curl`
//!    (the form goes to curl on its standard input, redirects are not followed), the same way
//!    `calendar/dav.rs` does HTTPS, so no HTTP library is needed.
//!
//! The refresh token is kept in the keyring ([`KeyringStore`]); the access token only in memory
//! ([`TokenSource`]), renewed shortly before it expires and when the server refuses it.
//! [`OAuthAuth`] is the IMAP side: `XOAUTH2` or `OAUTHBEARER` (RFC 7628). Tokens are never
//! logged and the `Debug` output of every type holding one hides it.
//!
//! There are no built-in client IDs: Shale's own registrations come later, and Thunderbird's
//! are never used. The client ID (and a client secret, where a provider wants one) is a setting
//! of the account.

use crate::calendar::dav::{self, Origin};
use crate::config::{MailAccount, OAuthProvider};
use crate::mail::imap_client::{base64_encode, Authenticator};
use crate::secret::Secret;
use crate::secrets::{self, MailSecret};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

/// How long the browser sign-in may take.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);
/// An access token is renewed when it has less than this left.
const EXPIRY_MARGIN: Duration = Duration::from_secs(60);
/// The button's name: error texts that need a new sign-in contain it (see [`needs_sign_in`]).
pub const SIGN_IN_BUTTON: &str = "Sign in in the browser";

// ---------------------------------------------------------------- errors

#[derive(Clone, Debug, PartialEq)]
pub enum OAuthError {
    /// There is no refresh token (never signed in, or it was cleared).
    NotSignedIn,
    /// The provider no longer accepts the refresh token (revoked, expired, password changed).
    Revoked,
    /// The person said no in the browser.
    Denied(String),
    Cancelled,
    TimedOut,
    /// The token endpoint refused (wrong client ID or secret, bad code …).
    Rejected(String),
    /// The token endpoint could not be reached or answered nonsense.
    Network(String),
    /// A setting is wrong (address, client ID …).
    Config(String),
    /// The browser could not be opened or the redirect listener could not start.
    Local(String),
}

impl fmt::Display for OAuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OAuthError::NotSignedIn => write!(f, "This account is not signed in yet: use “{SIGN_IN_BUTTON}” in the account settings."),
            OAuthError::Revoked => write!(f, "The sign-in expired or was revoked. {SIGN_IN_AGAIN}"),
            OAuthError::Denied(t) | OAuthError::Rejected(t) | OAuthError::Network(t) | OAuthError::Config(t) | OAuthError::Local(t) => f.write_str(t),
            OAuthError::Cancelled => f.write_str("The sign-in was cancelled."),
            OAuthError::TimedOut => f.write_str("The sign-in took too long and was given up. Try again."),
        }
    }
}

impl std::error::Error for OAuthError {}

/// What an error text says when the old sign-in is dead.
pub const SIGN_IN_AGAIN: &str = "Sign in again with the button in the account settings.";

/// Does this error text say the account needs a (new) browser sign-in?
pub fn needs_sign_in(text: &str) -> bool {
    text.contains(SIGN_IN_BUTTON) || text.contains("Sign in again with the button")
}

// ---------------------------------------------------------------- settings

/// The preset endpoints of a provider: (authorization, token, scope).
pub fn preset(provider: OAuthProvider) -> Option<(&'static str, &'static str, &'static str)> {
    match provider {
        OAuthProvider::Google => Some(("https://accounts.google.com/o/oauth2/v2/auth", "https://oauth2.googleapis.com/token", "https://mail.google.com/")),
        OAuthProvider::Microsoft => Some((
            "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            "https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send offline_access",
        )),
        OAuthProvider::Custom => None,
    }
}

/// The IMAP server of a preset provider: (host, port).
pub fn preset_server(provider: OAuthProvider) -> Option<(&'static str, u16)> {
    match provider {
        OAuthProvider::Google => Some(("imap.gmail.com", 993)),
        OAuthProvider::Microsoft => Some(("outlook.office365.com", 993)),
        OAuthProvider::Custom => None,
    }
}

/// The SMTP server of a preset provider: (host, port, security, the server keeps sent mail itself) (MAIL-120).
pub fn preset_smtp(provider: OAuthProvider) -> Option<(&'static str, u16, crate::config::MailSecurity, bool)> {
    match provider {
        OAuthProvider::Google => Some(("smtp.gmail.com", 465, crate::config::MailSecurity::ImplicitTls, true)),
        OAuthProvider::Microsoft => Some(("smtp.office365.com", 587, crate::config::MailSecurity::StartTls, false)),
        OAuthProvider::Custom => None,
    }
}

impl OAuthProvider {
    pub fn label(self) -> &'static str {
        match self {
            OAuthProvider::Google => "Google",
            OAuthProvider::Microsoft => "Microsoft",
            OAuthProvider::Custom => "Custom",
        }
    }
}

/// Everything the flow needs to know about an account.
#[derive(Clone, PartialEq)]
pub struct OAuthConfig {
    pub provider: OAuthProvider,
    pub auth_url: String,
    pub token_url: String,
    pub scope: String,
    pub client_id: String,
    pub client_secret: Option<Secret>,
    /// The user name, offered to the sign-in page as a hint.
    pub login_hint: String,
}

impl fmt::Debug for OAuthConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthConfig")
            .field("provider", &self.provider)
            .field("auth_url", &self.auth_url)
            .field("token_url", &self.token_url)
            .field("client_id", &self.client_id)
            .field("client_secret", &self.client_secret.as_ref().map(|_| "***"))
            .finish()
    }
}

/// An endpoint address is acceptable: `https://`, or `http://` on this computer for Custom only.
pub fn check_endpoint(url: &str, custom: bool) -> Result<(), String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("an address is missing".into());
    }
    let o = Origin::parse(url).map_err(|e| format!("{url}: {e}"))?;
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) || url.contains('#') {
        return Err(format!("{url}: the address must not contain spaces or a fragment"));
    }
    match o.scheme.as_str() {
        "https" => Ok(()),
        "http" if custom && o.is_local() => Ok(()),
        _ => Err(format!("{url}: only https:// addresses are used (http:// only for the Custom provider on this computer)")),
    }
}

impl OAuthConfig {
    /// The settings of an OAuth2 account; `client_secret` comes from the keyring or was typed.
    pub fn for_account(a: &MailAccount, client_secret: Option<Secret>) -> Result<OAuthConfig, OAuthError> {
        let bad = |t: String| OAuthError::Config(t);
        let client_id = a.oauth_client_id.trim().to_string();
        if client_id.is_empty() {
            return Err(bad("Enter the client ID of your OAuth2 app first (see the README: Google and Microsoft each need one).".into()));
        }
        if client_id.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(bad("The client ID must not contain spaces or line breaks.".into()));
        }
        let (auth_url, token_url, scope) = match preset(a.oauth_provider) {
            Some((x, y, z)) => (x.to_string(), y.to_string(), z.to_string()),
            None => (a.oauth_auth_url.trim().to_string(), a.oauth_token_url.trim().to_string(), a.oauth_scope.trim().to_string()),
        };
        let custom = a.oauth_provider == OAuthProvider::Custom;
        check_endpoint(&auth_url, custom).map_err(|e| bad(format!("Authorization address: {e}")))?;
        check_endpoint(&token_url, custom).map_err(|e| bad(format!("Token address: {e}")))?;
        if scope.is_empty() || scope.chars().any(char::is_control) {
            return Err(bad("Enter the scope (for example “mail”).".into()));
        }
        let client_secret = client_secret.filter(|s| !s.is_empty());
        Ok(OAuthConfig { provider: a.oauth_provider, auth_url, token_url, scope, client_id, client_secret, login_hint: a.user.clone() })
    }

    fn allow_local_http(&self) -> bool {
        self.provider == OAuthProvider::Custom
    }
}

// ---------------------------------------------------------------- small encoders

/// Append `s` percent-encoded. No intermediate strings: `out` can be a wiped one.
fn pct_push(out: &mut String, s: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 15) as usize] as char);
        }
    }
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => match (char::from(b[i + 1]).to_digit(16), char::from(b[i + 2]).to_digit(16)) {
                (Some(h), Some(l)) => {
                    out.push((h * 16 + l) as u8);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `a=b&c=d`, every part percent-encoded. The form can hold a client secret or a token, so it is
/// built in one buffer of the largest possible size (it never reallocates) that is wiped on drop.
pub fn form_body(params: &[(&str, &str)]) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::with_capacity(params.iter().map(|(k, v)| 3 * (k.len() + v.len()) + 2).sum()));
    for (i, (k, v)) in params.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        pct_push(&mut out, k);
        out.push('=');
        pct_push(&mut out, v);
    }
    out
}

fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&').filter(|p| !p.is_empty()).map(|p| p.split_once('=').map(|(k, v)| (pct_decode(k), pct_decode(v))).unwrap_or((pct_decode(p), String::new()))).collect()
}

/// Base64 with the URL alphabet and no padding (RFC 4648 §5), as PKCE wants it.
pub fn base64url(data: &[u8]) -> String {
    base64_encode(data).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

pub fn sha256(data: &[u8]) -> Vec<u8> {
    let suite = rustls::crypto::ring::cipher_suite::TLS13_AES_128_GCM_SHA256;
    match suite.tls13() {
        Some(s) => s.common.hash_provider.hash(data).as_ref().to_vec(),
        None => Vec::new(),
    }
}

/// Random bytes from the operating system's generator (through the TLS library's provider).
pub fn random_bytes(n: usize) -> Result<Vec<u8>, OAuthError> {
    let mut buf = vec![0u8; n];
    rustls::crypto::ring::default_provider().secure_random.fill(&mut buf).map_err(|_| OAuthError::Local("no secure random numbers available".into()))?;
    Ok(buf)
}

// ---------------------------------------------------------------- PKCE and the sign-in address

/// The PKCE challenge of a verifier: `BASE64URL(SHA256(verifier))`.
pub fn pkce_challenge(verifier: &str) -> String {
    base64url(&sha256(verifier.as_bytes()))
}

/// The address to open in the browser.
pub fn authorization_url(cfg: &OAuthConfig, redirect_uri: &str, state: &str, challenge: &str) -> String {
    let mut params = vec![("response_type", "code"), ("client_id", cfg.client_id.as_str()), ("redirect_uri", redirect_uri), ("scope", cfg.scope.as_str()), ("state", state), ("code_challenge", challenge), ("code_challenge_method", "S256")];
    if !cfg.login_hint.is_empty() {
        params.push(("login_hint", cfg.login_hint.as_str()));
    }
    if cfg.provider == OAuthProvider::Google {
        // without these Google may not hand out a refresh token
        params.push(("access_type", "offline"));
        params.push(("prompt", "consent"));
    }
    let sep = if cfg.auth_url.contains('?') { '&' } else { '?' };
    format!("{}{sep}{}", cfg.auth_url, form_body(&params).as_str())
}

// ---------------------------------------------------------------- the callback

#[derive(Debug, PartialEq)]
pub enum Callback {
    /// The authorization code.
    Code(String),
    /// The provider reported an error for our request (the person said no, usually).
    Denied(String),
    /// Not our answer: HTTP status and why. The listener goes on.
    Reject(u16, &'static str),
}

fn same_bytes(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(200).collect()
}

/// Judge one request that reached the listener. `head` is the request line and the headers.
pub fn parse_callback(head: &str, port: u16, state: &str) -> Callback {
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, target) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
    if method != "GET" {
        return Callback::Reject(405, "only GET is answered");
    }
    let host = lines.find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("host")).map(|(_, v)| v.trim().to_string()));
    if host.as_deref() != Some(format!("127.0.0.1:{port}").as_str()) {
        return Callback::Reject(400, "wrong Host header");
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/" {
        return Callback::Reject(404, "wrong path");
    }
    let q = parse_query(query);
    let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    if !get("state").is_some_and(|s| same_bytes(s, state)) {
        return Callback::Reject(400, "the state does not match");
    }
    if let Some(e) = get("error") {
        let text = match e {
            "access_denied" => "The sign-in was cancelled in the browser.".to_string(),
            _ => format!("The provider refused the sign-in: {}{}", clean(e), get("error_description").map(|d| format!(" ({})", clean(d))).unwrap_or_default()),
        };
        return Callback::Denied(text);
    }
    match get("code").filter(|c| !c.is_empty()) {
        Some(c) => Callback::Code(c.to_string()),
        None => Callback::Reject(400, "no code in the answer"),
    }
}

fn reply(stream: &mut TcpStream, status: u16, text: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let body = format!("<!doctype html><meta charset=utf-8><title>Shale</title><p>{text}</p>");
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

/// Read the head of a request (up to the empty line, at most 16 KB).
fn read_head(stream: &mut TcpStream) -> Option<String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while buf.len() < 16 * 1024 && !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let end = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    Some(String::from_utf8_lossy(&buf[..end]).into_owned())
}

// ---------------------------------------------------------------- the flow

/// A sign-in that has started: the listener is bound and the address is ready.
pub struct Pending {
    listener: TcpListener,
    pub port: u16,
    pub redirect_uri: String,
    state: String,
    verifier: String,
    /// What to open in the browser.
    pub url: String,
}

impl fmt::Debug for Pending {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pending").field("port", &self.port).finish()
    }
}

/// Bind the redirect listener (127.0.0.1 only, a random port) and build the sign-in address.
pub fn begin(cfg: &OAuthConfig) -> Result<Pending, OAuthError> {
    let listener = TcpListener::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))).map_err(|e| OAuthError::Local(format!("could not listen for the browser's answer: {e}")))?;
    listener.set_nonblocking(true).map_err(|e| OAuthError::Local(e.to_string()))?;
    let port = listener.local_addr().map_err(|e| OAuthError::Local(e.to_string()))?.port();
    let verifier = base64url(&random_bytes(32)?);
    let state = base64url(&random_bytes(24)?);
    let redirect_uri = format!("http://127.0.0.1:{port}/");
    let url = authorization_url(cfg, &redirect_uri, &state, &pkce_challenge(&verifier));
    Ok(Pending { listener, port, redirect_uri, state, verifier, url })
}

impl Pending {
    /// Wait for the browser's answer: exactly one valid callback ends it. Anything else is
    /// answered with an error page and ignored.
    pub fn wait_for_code(&self, cancel: &AtomicBool, timeout: Duration) -> Result<String, OAuthError> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(OAuthError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(OAuthError::TimedOut);
            }
            match self.listener.accept() {
                Ok((mut s, _)) => {
                    let _ = s.set_nonblocking(false);
                    let Some(head) = read_head(&mut s) else {
                        reply(&mut s, 400, "Not a valid sign-in answer.");
                        continue;
                    };
                    match parse_callback(&head, self.port, &self.state) {
                        Callback::Code(c) => {
                            reply(&mut s, 200, "Shale: you are signed in. You can close this tab.");
                            return Ok(c);
                        }
                        Callback::Denied(t) => {
                            reply(&mut s, 200, "Shale: the sign-in was not completed. You can close this tab.");
                            return Err(OAuthError::Denied(t));
                        }
                        Callback::Reject(status, _) => reply(&mut s, status, "Not a valid sign-in answer."),
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(25)),
                Err(e) => return Err(OAuthError::Local(format!("the listener failed: {e}"))),
            }
        }
    }
}

// ---------------------------------------------------------------- tokens

/// What the token endpoint handed out.
#[derive(Clone, PartialEq)]
pub struct Tokens {
    pub access_token: Secret,
    pub refresh_token: Option<Secret>,
    pub expires_in: u64,
    /// Who signed in, when the answer says (the `email` of the ID token). Only for display.
    pub user: Option<String>,
}

impl fmt::Debug for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tokens").field("access_token", &"***").field("refresh_token", &self.refresh_token.as_ref().map(|_| "***")).field("expires_in", &self.expires_in).field("user", &self.user).finish()
    }
}

/// The person named in an ID token's payload (not verified: it is only shown, never trusted).
fn user_of_id_token(jwt: &str) -> Option<String> {
    let payload = jwt.split('.').nth(1)?;
    let v: Value = serde_json::from_slice(&crate::mail::base64_decode(payload.as_bytes())).ok()?;
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(clean).filter(|s| !s.is_empty());
    // `sub` is often a number: shown only when it is an address
    text("email").or_else(|| text("preferred_username")).or_else(|| text("upn")).or_else(|| text("sub").filter(|s| s.contains('@')))
}

/// Read a token endpoint's answer. `refreshing`: an `invalid_grant` then means the refresh token
/// is dead (revoked or expired), not that a code was wrong.
pub fn parse_token_response(status: u16, body: &str, refreshing: bool) -> Result<Tokens, OAuthError> {
    /// The tokens are read straight into `Secret`s: no `Value` tree that keeps them in unwiped strings.
    #[derive(serde::Deserialize)]
    struct Raw {
        access_token: Option<Secret>,
        refresh_token: Option<Secret>,
        token_type: Option<String>,
        expires_in: Option<Value>,
        id_token: Option<Secret>,
        error: Option<String>,
        error_description: Option<String>,
    }
    let v: Raw = serde_json::from_str(body).map_err(|_| OAuthError::Network(format!("the token endpoint's answer could not be read (status {status})")))?;
    if let Some(e) = v.error.as_deref() {
        let desc = v.error_description.as_deref().map(clean).unwrap_or_default();
        return Err(match e {
            "invalid_grant" if refreshing => OAuthError::Revoked,
            "invalid_grant" => OAuthError::Rejected(format!("The provider did not accept the sign-in code (invalid_grant). {desc}").trim().to_string()),
            "invalid_client" | "unauthorized_client" => OAuthError::Rejected(format!("The provider does not accept this client ID or secret ({}). {desc}", clean(e)).trim().to_string()),
            other => OAuthError::Rejected(format!("The provider refused: {}. {desc}", clean(other)).trim().to_string()),
        });
    }
    if !(200..300).contains(&status) {
        return Err(OAuthError::Rejected(format!("the token endpoint answered with status {status}")));
    }
    let access = v.access_token.filter(|t| !t.is_empty()).ok_or_else(|| OAuthError::Rejected("the token endpoint sent no access token".into()))?;
    if let Some(t) = v.token_type.as_deref() {
        if !t.eq_ignore_ascii_case("bearer") {
            return Err(OAuthError::Rejected(format!("the token endpoint sent a “{}” token, but only bearer tokens are used", clean(t))));
        }
    }
    let expires_in = match &v.expires_in {
        Some(Value::Number(n)) => n.as_u64().or_else(|| n.as_f64().map(|f| f.max(0.0) as u64)),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
    .unwrap_or(3600);
    Ok(Tokens { access_token: access, refresh_token: v.refresh_token.filter(|t| !t.is_empty()), expires_in, user: v.id_token.as_ref().and_then(|t| user_of_id_token(t.expose())) })
}

fn post_form(cfg: &OAuthConfig, params: &[(&str, &str)], refreshing: bool) -> Result<Tokens, OAuthError> {
    check_endpoint(&cfg.token_url, cfg.allow_local_http()).map_err(OAuthError::Config)?;
    let body = form_body(params);
    let nobody = dav::Account { url: String::new(), username: String::new(), password: Secret::default() };
    let r = dav::request_opts("POST", &cfg.token_url, &["Content-Type: application/x-www-form-urlencoded", "Accept: application/json"], &body, &nobody, false).map_err(OAuthError::Network)?;
    // the answer holds the tokens: its text is wiped once read
    let answer = Zeroizing::new(r.body);
    parse_token_response(r.status, &answer, refreshing)
}

/// Trade the authorization code for tokens.
pub fn exchange_code(cfg: &OAuthConfig, code: &str, redirect_uri: &str, verifier: &str) -> Result<Tokens, OAuthError> {
    let mut p = vec![("grant_type", "authorization_code"), ("code", code), ("redirect_uri", redirect_uri), ("client_id", cfg.client_id.as_str()), ("code_verifier", verifier)];
    if let Some(s) = &cfg.client_secret {
        p.push(("client_secret", s.expose()));
    }
    post_form(cfg, &p, false)
}

/// Get a new access token with the refresh token.
pub fn refresh(cfg: &OAuthConfig, refresh_token: &str) -> Result<Tokens, OAuthError> {
    let mut p = vec![("grant_type", "refresh_token"), ("refresh_token", refresh_token), ("client_id", cfg.client_id.as_str())];
    if let Some(s) = &cfg.client_secret {
        p.push(("client_secret", s.expose()));
    }
    post_form(cfg, &p, true)
}

// ---------------------------------------------------------------- keeping tokens

/// Where the refresh token lives between runs.
pub trait RefreshStore: Send + Sync {
    fn load(&self) -> Option<Secret>;
    /// False if it could not be kept (no keyring).
    fn save(&self, token: &str) -> bool;
    fn clear(&self);
}

/// The system keyring (`secrets.rs`).
pub struct KeyringStore {
    pub account_id: String,
}

impl RefreshStore for KeyringStore {
    fn load(&self) -> Option<Secret> {
        secrets::mail_secret(&self.account_id, MailSecret::RefreshToken)
    }
    fn save(&self, token: &str) -> bool {
        secrets::set_mail_secret(&self.account_id, MailSecret::RefreshToken, token)
    }
    fn clear(&self) {
        secrets::clear_mail_secret(&self.account_id, MailSecret::RefreshToken);
    }
}

struct Inner {
    access: Option<(Secret, Instant)>,
    refresh: Option<Secret>,
    loaded: bool,
    /// The refresh token in memory is newer than the one in the keyring (it could not be saved).
    unsaved: bool,
    /// The person has been told about `unsaved`.
    told: bool,
}

/// The access token of one account: kept in memory, renewed when it is about to expire or when
/// the server refused it. One refresh at a time, shared by all connections of the account.
pub struct TokenSource {
    cfg: OAuthConfig,
    store: Arc<dyn RefreshStore>,
    inner: Mutex<Inner>,
    refreshes: std::sync::atomic::AtomicUsize,
}

impl fmt::Debug for TokenSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenSource").field("cfg", &self.cfg).finish()
    }
}

impl TokenSource {
    pub fn new(cfg: OAuthConfig, store: Arc<dyn RefreshStore>) -> TokenSource {
        TokenSource { cfg, store, inner: Mutex::new(Inner { access: None, refresh: None, loaded: false, unsaved: false, told: false }), refreshes: std::sync::atomic::AtomicUsize::new(0) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// How many times the token endpoint was asked for a new access token (tests).
    #[cfg(test)]
    pub fn refresh_count(&self) -> usize {
        self.refreshes.load(Ordering::SeqCst)
    }

    /// A usable access token: the one in memory while it has time left, else a new one.
    pub fn access_token(&self) -> Result<Secret, OAuthError> {
        let mut g = self.lock();
        if let Some((t, exp)) = &g.access {
            if Instant::now() + EXPIRY_MARGIN < *exp {
                return Ok(t.clone());
            }
        }
        self.refresh_locked(&mut g)
    }

    fn refresh_locked(&self, g: &mut Inner) -> Result<Secret, OAuthError> {
        let _flow = tracing::info_span!("flow.mail_oauth_renew", peer = "mail").entered();
        if !g.loaded {
            g.refresh = tracing::info_span!("flow.mail_oauth_renew.load_refresh_token", peer = "keyring").in_scope(|| self.store.load());
            g.loaded = true;
        }
        let Some(rt) = g.refresh.clone() else { return Err(OAuthError::NotSignedIn) };
        // a renewed token the keyring did not take earlier: try again now
        if g.unsaved && self.store.save(rt.expose()) {
            g.unsaved = false;
            g.told = false;
        }
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        match tracing::info_span!("flow.mail_oauth_renew.token_request", peer = "provider").in_scope(|| refresh(&self.cfg, rt.expose())) {
            Ok(t) => {
                g.access = Some((t.access_token.clone(), Instant::now() + Duration::from_secs(t.expires_in)));
                if let Some(new) = t.refresh_token.filter(|n| *n != rt) {
                    // the provider rotates refresh tokens: the old one is dead
                    if tracing::info_span!("flow.mail_oauth_renew.save_refresh_token", peer = "keyring").in_scope(|| self.store.save(new.expose())) {
                        g.unsaved = false;
                        g.told = false;
                    } else {
                        // the keyring still holds the dead one: keep the new one in memory, try
                        // again at the next refresh and tell the person once (SEC-012)
                        g.unsaved = true;
                    }
                    g.refresh = Some(new);
                }
                Ok(t.access_token)
            }
            Err(OAuthError::Revoked) => {
                g.access = None;
                g.refresh = None;
                tracing::info_span!("flow.mail_oauth_renew.clear_refresh_token", peer = "keyring").in_scope(|| self.store.clear());
                Err(OAuthError::Revoked)
            }
            Err(e) => Err(e),
        }
    }

    /// The server refused the access token: the next call gets a new one.
    pub fn reject(&self) {
        self.lock().access = None;
    }

    /// Keep the tokens of a finished sign-in. Returns whether the keyring has the refresh token
    /// (if not, it is remembered for this run only).
    pub fn adopt(&self, t: &Tokens) -> bool {
        let mut g = self.lock();
        g.loaded = true;
        g.access = Some((t.access_token.clone(), Instant::now() + Duration::from_secs(t.expires_in)));
        match &t.refresh_token {
            Some(r) => {
                g.refresh = Some(r.clone());
                let kept = self.store.save(r.expose());
                g.unsaved = !kept;
                g.told = !kept; // the sign-in dialog says so itself
                kept
            }
            None => false,
        }
    }

    /// True once after a renewed refresh token could not be saved in the keyring.
    fn take_unsaved_notice(&self) -> bool {
        let mut g = self.lock();
        let tell = g.unsaved && !g.told;
        if tell {
            g.told = true;
        }
        tell
    }

    /// Is there a refresh token (in memory or the keyring)?
    pub fn signed_in(&self) -> bool {
        let mut g = self.lock();
        if !g.loaded {
            g.refresh = self.store.load();
            g.loaded = true;
        }
        g.refresh.is_some()
    }
}

type Registry = Mutex<HashMap<String, Arc<TokenSource>>>;

fn registry() -> &'static Registry {
    static R: std::sync::OnceLock<Registry> = std::sync::OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The token source of an account, shared by everything that connects for it (so a sync with
/// four connections refreshes once). A new one replaces it when the account's settings changed.
pub fn shared_source(account_id: &str, cfg: OAuthConfig) -> Arc<TokenSource> {
    let mut r = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = r.get(account_id).filter(|s| s.cfg == cfg) {
        return s.clone();
    }
    let s = Arc::new(TokenSource::new(cfg, Arc::new(KeyringStore { account_id: account_id.to_string() })));
    r.insert(account_id.to_string(), s.clone());
    s
}

/// The accounts whose renewed sign-in could not be saved in the keyring, each reported once (SEC-012).
pub fn take_unsaved_notices() -> Vec<String> {
    let r = registry().lock().unwrap_or_else(|e| e.into_inner());
    r.iter().filter(|(_, s)| s.take_unsaved_notice()).map(|(id, _)| id.clone()).collect()
}

/// Forget the in-memory tokens of an account (it was removed).
pub fn forget_source(account_id: &str) {
    registry().lock().unwrap_or_else(|e| e.into_inner()).remove(account_id);
}

/// Is the account signed in? Reads the keyring: run it in a job.
pub fn is_signed_in(account: &MailAccount) -> bool {
    match registry().lock().unwrap_or_else(|e| e.into_inner()).get(&account.id).cloned() {
        Some(s) => s.signed_in(),
        None => secrets::mail_secret(&account.id, MailSecret::RefreshToken).is_some(),
    }
}

/// The result of a sign-in.
#[derive(Debug, Clone, PartialEq)]
pub struct SignedIn {
    /// Who signed in, when the provider says so.
    pub user: Option<String>,
    /// The refresh token is in the keyring (else it only lives until Shale quits).
    pub kept: bool,
}

/// Cancel was asked for: the sign-in ends here and keeps nothing (JOB-032).
fn stop_if_cancelled(cancel: &AtomicBool) -> Result<(), OAuthError> {
    if cancel.load(Ordering::SeqCst) {
        Err(OAuthError::Cancelled)
    } else {
        Ok(())
    }
}

/// The whole browser sign-in of an account. Blocks (up to `timeout`): run it as a job.
/// `shown` gets the sign-in address (so it can be offered when no browser opens).
pub fn sign_in(account: &MailAccount, client_secret: Option<Secret>, cancel: &AtomicBool, timeout: Duration, shown: &dyn Fn(&str)) -> Result<SignedIn, OAuthError> {
    let _flow = tracing::info_span!("flow.mail_oauth", peer = "signin").entered();
    let cfg = OAuthConfig::for_account(account, client_secret)?;
    let pending = tracing::info_span!("flow.mail_oauth.begin", peer = "loopback").in_scope(|| begin(&cfg))?;
    shown(&pending.url);
    // a browser that cannot be opened is not the end: the address is shown, and the listener waits
    let opened = tracing::info_span!("flow.mail_oauth.open_browser", peer = "browser").in_scope(|| crate::opener::open_signin_url(&pending.url, cfg.allow_local_http()));
    if let Err(e) = &opened {
        if e.starts_with("The sign-in address") {
            return Err(OAuthError::Config(e.clone()));
        }
    }
    let code = tracing::info_span!("flow.mail_oauth.wait_for_code", peer = "browser").in_scope(|| pending.wait_for_code(cancel, timeout))?;
    // Cancel, also pressed just as the code came: no exchange (JOB-032)
    stop_if_cancelled(cancel)?;
    let tokens = tracing::info_span!("flow.mail_oauth.exchange_code", peer = "provider").in_scope(|| exchange_code(&cfg, &code, &pending.redirect_uri, &pending.verifier))?;
    if tokens.refresh_token.is_none() {
        return Err(OAuthError::Rejected("The provider sent no refresh token, so the sign-in could not be kept (with Microsoft the scope must include offline_access).".into()));
    }
    // all or nothing (JOB-032): Cancel until here drops the tokens, and from here on the sign-in is kept whole
    stop_if_cancelled(cancel)?;
    let kept = tracing::info_span!("flow.mail_oauth.adopt", peer = "keyring").in_scope(|| shared_source(&account.id, cfg).adopt(&tokens));
    Ok(SignedIn { user: tokens.user, kept })
}

// ---------------------------------------------------------------- IMAP: XOAUTH2 and OAUTHBEARER

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mech {
    XOAuth2,
    OAuthBearer,
}

/// Signs in to IMAP with an access token: `OAUTHBEARER` if the server offers it, else `XOAUTH2`.
pub struct OAuthAuth {
    user: String,
    token: Secret,
    host: String,
    port: u16,
    mech: Mech,
}

impl OAuthAuth {
    pub fn new(user: &str, token: &str, host: &str, port: u16) -> OAuthAuth {
        OAuthAuth { user: user.into(), token: Secret::new(token), host: host.into(), port, mech: Mech::XOAuth2 }
    }
}

impl fmt::Debug for OAuthAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OAuthAuth({}, {:?}, ***)", self.user, self.mech)
    }
}

/// `user=<user>\x01auth=Bearer <token>\x01\x01` (the XOAUTH2 initial response).
pub fn xoauth2_response(user: &str, token: &str) -> Zeroizing<Vec<u8>> {
    bytes_of(&["user=", user, "\x01auth=Bearer ", token, "\x01\x01"])
}

/// The parts joined into one byte buffer of exactly the needed size, wiped when dropped.
fn bytes_of(parts: &[&str]) -> Zeroizing<Vec<u8>> {
    let mut v = Zeroizing::new(Vec::with_capacity(parts.iter().map(|p| p.len()).sum()));
    for p in parts {
        v.extend_from_slice(p.as_bytes());
    }
    v
}

/// The OAUTHBEARER initial response (RFC 7628 §3.1). `=` and `,` in the user name are escaped.
pub fn oauthbearer_response(user: &str, host: &str, port: u16, token: &str) -> Zeroizing<Vec<u8>> {
    bytes_of(&["n,a=", &user.replace('=', "=3D").replace(',', "=2C"), ",\x01host=", host, "\x01port=", &port.to_string(), "\x01auth=Bearer ", token, "\x01\x01"])
}

impl Authenticator for OAuthAuth {
    fn select_mechanism(&mut self, offered: &dyn Fn(&str) -> bool) {
        self.mech = if offered("OAUTHBEARER") { Mech::OAuthBearer } else { Mech::XOAuth2 };
    }
    fn mechanism(&self) -> &str {
        match self.mech {
            Mech::XOAuth2 => "XOAUTH2",
            Mech::OAuthBearer => "OAUTHBEARER",
        }
    }
    fn initial_response(&mut self) -> Zeroizing<Vec<u8>> {
        match self.mech {
            Mech::XOAuth2 => xoauth2_response(&self.user, self.token.expose()),
            Mech::OAuthBearer => oauthbearer_response(&self.user, &self.host, self.port, self.token.expose()),
        }
    }
    /// The server's reason for refusing the token arrives as a challenge; the specifications
    /// say to answer it with an empty line (XOAUTH2) or a single `\x01` (OAUTHBEARER), and the
    /// server then ends with `NO`.
    fn respond(&mut self, _challenge: &[u8]) -> Vec<u8> {
        match self.mech {
            Mech::XOAuth2 => Vec::new(),
            Mech::OAuthBearer => vec![1],
        }
    }
}

/// An `AuthFactory` that signs in with the access token of a [`TokenSource`].
pub struct OAuthFactory {
    pub user: String,
    pub host: String,
    pub port: u16,
    pub source: Arc<TokenSource>,
}

impl crate::mail::imap_sync::AuthFactory for OAuthFactory {
    fn authenticator(&self) -> Result<Box<dyn Authenticator>, String> {
        let token = self.source.access_token().map_err(|e| e.to_string())?;
        Ok(Box::new(OAuthAuth::new(&self.user, token.expose(), &self.host, self.port)))
    }
    /// The server refused the token: forget it, so that the connection is tried once more with
    /// a new one.
    fn token_rejected(&self) -> bool {
        self.source.reject();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail::oauth_fake_tests::{http_get, FakeOAuth};
    use std::sync::atomic::AtomicBool;

    fn has_curl() -> bool {
        std::process::Command::new("curl").arg("--version").output().is_ok()
    }

    fn custom(f: &FakeOAuth) -> MailAccount {
        MailAccount {
            id: "oauth-t".into(),
            user: "ada@example.com".into(),
            oauth_provider: OAuthProvider::Custom,
            oauth_client_id: "shale-test".into(),
            oauth_auth_url: format!("http://127.0.0.1:{}/authorize", f.port),
            oauth_token_url: format!("http://127.0.0.1:{}/token", f.port),
            oauth_scope: "mail".into(),
            ..MailAccount::default()
        }
    }

    struct MemStore(Mutex<Option<Secret>>);
    impl RefreshStore for MemStore {
        fn load(&self) -> Option<Secret> {
            self.0.lock().unwrap().clone()
        }
        fn save(&self, t: &str) -> bool {
            *self.0.lock().unwrap() = Some(Secret::new(t));
            true
        }
        fn clear(&self) {
            *self.0.lock().unwrap() = None;
        }
    }

    /// A store that refuses to save while `refuse` is set.
    struct FlakyStore {
        token: Mutex<Option<Secret>>,
        refuse: std::sync::atomic::AtomicBool,
    }
    impl RefreshStore for FlakyStore {
        fn load(&self) -> Option<Secret> {
            self.token.lock().unwrap().clone()
        }
        fn save(&self, t: &str) -> bool {
            if self.refuse.load(Ordering::SeqCst) {
                return false;
            }
            *self.token.lock().unwrap() = Some(Secret::new(t));
            true
        }
        fn clear(&self) {
            *self.token.lock().unwrap() = None;
        }
    }

    fn cfg_for(f: &FakeOAuth) -> OAuthConfig {
        OAuthConfig::for_account(&custom(f), None).unwrap()
    }

    // spec: SEC-012
    #[test]
    fn a_renewed_token_the_keyring_cannot_keep_is_kept_in_memory_saved_later_and_reported_once() {
        if !has_curl() {
            return;
        }
        let f = FakeOAuth::start();
        let cfg = cfg_for(&f);
        let rt = f.new_refresh_token();
        let store = Arc::new(FlakyStore { token: Mutex::new(Some(Secret::new(&rt))), refuse: std::sync::atomic::AtomicBool::new(true) });
        let src = TokenSource::new(cfg, store.clone());
        f.state.lock().unwrap().rotate = true;
        src.access_token().unwrap();
        assert_eq!(store.load().unwrap(), rt, "the keyring still has the old token");
        assert!(src.take_unsaved_notice(), "the person is told");
        assert!(!src.take_unsaved_notice(), "only once");
        // the next refresh uses the new token from memory (the old one is dead) and tries to save again
        src.reject();
        src.access_token().unwrap();
        assert!(!src.take_unsaved_notice(), "still only once");
        store.refuse.store(false, Ordering::SeqCst);
        src.reject();
        src.access_token().unwrap();
        assert_ne!(store.load().unwrap(), rt, "saved as soon as the keyring takes it");
        assert!(!src.take_unsaved_notice());
    }

    // spec: MAIL-080
    #[test]
    fn pkce_matches_the_rfc_7636_example_and_the_verifier_is_long_and_random() {
        // RFC 7636 appendix B
        assert_eq!(pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        // SHA-256 of "abc" (FIPS 180-2)
        assert_eq!(sha256(b"abc").iter().map(|b| format!("{b:02x}")).collect::<String>(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let cfg = OAuthConfig::for_account(&MailAccount { oauth_client_id: "id".into(), ..MailAccount::default() }, None).unwrap();
        let a = begin(&cfg).unwrap();
        let b = begin(&cfg).unwrap();
        assert!(a.verifier.len() >= 43 && a.verifier.len() <= 128, "RFC 7636 §4.1");
        assert_ne!(a.verifier, b.verifier);
        assert_ne!(a.state, b.state);
        assert_ne!(a.port, b.port);
    }

    // spec: MAIL-080, MAIL-085
    #[test]
    fn the_sign_in_address_carries_pkce_state_scope_and_the_loopback_redirect() {
        let a = MailAccount { oauth_client_id: "cid".into(), user: "ada@gmail.com".into(), ..MailAccount::default() };
        let cfg = OAuthConfig::for_account(&a, None).unwrap();
        let p = begin(&cfg).unwrap();
        let (base, q) = p.url.split_once('?').unwrap();
        assert_eq!(base, "https://accounts.google.com/o/oauth2/v2/auth");
        let q = parse_query(q);
        let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
        assert_eq!(get("response_type"), "code");
        assert_eq!(get("client_id"), "cid");
        assert_eq!(get("redirect_uri"), format!("http://127.0.0.1:{}/", p.port));
        assert_eq!(get("scope"), "https://mail.google.com/");
        assert_eq!(get("code_challenge_method"), "S256");
        assert_eq!(get("code_challenge"), pkce_challenge(&p.verifier));
        assert_eq!(get("state"), p.state);
        assert_eq!(get("login_hint"), "ada@gmail.com");
        assert_eq!(get("access_type"), "offline");
        // the verifier itself never goes into the address
        assert!(!p.url.contains(&p.verifier));
        // Microsoft's preset
        let m = OAuthConfig::for_account(&MailAccount { oauth_provider: OAuthProvider::Microsoft, oauth_client_id: "x".into(), ..MailAccount::default() }, None).unwrap();
        assert_eq!(m.auth_url, "https://login.microsoftonline.com/common/oauth2/v2.0/authorize");
        assert_eq!(m.token_url, "https://login.microsoftonline.com/common/oauth2/v2.0/token");
        assert_eq!(m.scope, "https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send offline_access");
        assert_eq!(preset_server(OAuthProvider::Google), Some(("imap.gmail.com", 993)));
        assert_eq!(preset_server(OAuthProvider::Microsoft), Some(("outlook.office365.com", 993)));
    }

    // spec: MAIL-085, MAIL-082
    #[test]
    fn there_is_no_built_in_client_id_and_endpoints_must_be_https_except_local_http_for_custom() {
        assert!(matches!(OAuthConfig::for_account(&MailAccount::default(), None), Err(OAuthError::Config(t)) if t.contains("client ID")));
        let mk = |p, auth: &str, tok: &str| MailAccount { oauth_provider: p, oauth_client_id: "x".into(), oauth_auth_url: auth.into(), oauth_token_url: tok.into(), oauth_scope: "mail".into(), ..MailAccount::default() };
        let ok = |a: MailAccount| OAuthConfig::for_account(&a, None).is_ok();
        assert!(ok(mk(OAuthProvider::Custom, "https://id.example.com/auth", "https://id.example.com/token")));
        assert!(ok(mk(OAuthProvider::Custom, "http://localhost:8080/auth", "http://127.0.0.1:8080/token")));
        assert!(!ok(mk(OAuthProvider::Custom, "http://id.example.com/auth", "https://id.example.com/token")), "plain http to another computer");
        assert!(!ok(mk(OAuthProvider::Custom, "https://id.example.com/auth", "http://id.example.com/token")));
        assert!(!ok(mk(OAuthProvider::Custom, "ftp://id.example.com/auth", "https://id.example.com/token")));
        assert!(!ok(mk(OAuthProvider::Custom, "https://u:p@id.example.com/auth", "https://id.example.com/token")), "no password in an address");
        assert!(!ok(mk(OAuthProvider::Custom, "", "https://id.example.com/token")));
        // the presets ignore whatever is typed in the custom fields: local http is never used with them
        let g = OAuthConfig::for_account(&mk(OAuthProvider::Google, "http://127.0.0.1/x", "http://127.0.0.1/y"), None).unwrap();
        assert!(g.auth_url.starts_with("https://accounts.google.com/") && g.token_url.starts_with("https://oauth2.googleapis.com/"));
        assert!(!g.allow_local_http());
        // and the browser is only opened for such addresses
        assert!(crate::opener::open_signin_url("http://127.0.0.1:1/auth", false).is_err());
        assert!(crate::opener::open_signin_url("http://example.com/auth", true).is_err());
        assert!(crate::opener::open_signin_url("file:///etc/passwd", true).is_err());
        assert!(crate::opener::open_signin_url("javascript:alert(1)", true).is_err());
    }

    // spec: MAIL-081
    #[test]
    fn a_callback_needs_the_path_the_state_the_host_and_a_code() {
        let req = |target: &str, host: &str| format!("GET {target} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: x\r\n\r\n");
        let h = "127.0.0.1:5000";
        assert_eq!(parse_callback(&req("/?code=abc%2Fdef&state=S1", h), 5000, "S1"), Callback::Code("abc/def".into()));
        assert_eq!(parse_callback(&req("/?state=S1&code=a+b", h), 5000, "S1"), Callback::Code("a b".into()));
        // state mismatch, missing state, wrong path, wrong host, wrong method, missing code
        assert!(matches!(parse_callback(&req("/?code=abc&state=OTHER", h), 5000, "S1"), Callback::Reject(400, _)));
        assert!(matches!(parse_callback(&req("/?code=abc", h), 5000, "S1"), Callback::Reject(400, _)));
        assert!(matches!(parse_callback(&req("/callback?code=abc&state=S1", h), 5000, "S1"), Callback::Reject(404, _)));
        assert!(matches!(parse_callback(&req("/favicon.ico", h), 5000, "S1"), Callback::Reject(404, _)));
        assert!(matches!(parse_callback(&req("/?code=abc&state=S1", "evil.example.com:5000"), 5000, "S1"), Callback::Reject(400, _)), "DNS rebinding");
        assert!(matches!(parse_callback(&req("/?code=abc&state=S1", "127.0.0.1:5001"), 5000, "S1"), Callback::Reject(400, _)));
        assert!(matches!(parse_callback("POST /?code=abc&state=S1 HTTP/1.1\r\nHost: 127.0.0.1:5000\r\n\r\n", 5000, "S1"), Callback::Reject(405, _)));
        assert!(matches!(parse_callback(&req("/?state=S1", h), 5000, "S1"), Callback::Reject(400, _)));
        assert!(matches!(parse_callback(&req("/?state=S1&code=", h), 5000, "S1"), Callback::Reject(400, _)));
        assert!(matches!(parse_callback("", 5000, "S1"), Callback::Reject(_, _)));
        // the person said no
        assert_eq!(parse_callback(&req("/?error=access_denied&state=S1", h), 5000, "S1"), Callback::Denied("The sign-in was cancelled in the browser.".into()));
        let Callback::Denied(t) = parse_callback(&req("/?error=server_error&error_description=Oops%0D%0Aboom&state=S1", h), 5000, "S1") else { panic!("not denied") };
        assert!(t.contains("server_error") && t.contains("Oopsboom"), "{t}");
        // an error answer for another request is not ours
        assert!(matches!(parse_callback(&req("/?error=access_denied&state=NOPE", h), 5000, "S1"), Callback::Reject(400, _)));
    }

    // spec: MAIL-081
    #[test]
    fn the_listener_takes_one_valid_callback_ignores_the_rest_and_can_be_cancelled_or_time_out() {
        let cfg = OAuthConfig::for_account(&MailAccount { oauth_client_id: "id".into(), ..MailAccount::default() }, None).unwrap();
        let p = begin(&cfg).unwrap();
        let cancel = AtomicBool::new(false);
        let (port, state) = (p.port, p.state.clone());
        let t = std::thread::spawn(move || {
            // junk first: a wrong state and a wrong path must not end the wait
            let (s1, _, _) = http_get(&format!("http://127.0.0.1:{port}/?code=evil&state=wrong"));
            let (s2, _, _) = http_get(&format!("http://127.0.0.1:{port}/steal?code=evil&state={state}"));
            let (s3, _, body) = http_get(&format!("http://127.0.0.1:{port}/?code=good&state={state}"));
            (s1, s2, s3, body)
        });
        assert_eq!(p.wait_for_code(&cancel, Duration::from_secs(10)), Ok("good".into()));
        let (s1, s2, s3, body) = t.join().unwrap();
        assert_eq!((s1, s2, s3), (400, 404, 200));
        assert!(body.contains("close this tab"));
        // it only listens on the loopback interface
        assert!(p.listener.local_addr().unwrap().ip().is_loopback());
        // timeout and cancel
        let q = begin(&cfg).unwrap();
        assert_eq!(q.wait_for_code(&cancel, Duration::from_millis(120)), Err(OAuthError::TimedOut));
        let cancel2 = Arc::new(AtomicBool::new(false));
        let c2 = cancel2.clone();
        let started = Instant::now();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            c2.store(true, Ordering::SeqCst);
        });
        assert_eq!(q.wait_for_code(&cancel2, Duration::from_secs(30)), Err(OAuthError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(5));
        // a denied sign-in ends it with the message
        let r = begin(&cfg).unwrap();
        let (port, state) = (r.port, r.state.clone());
        std::thread::spawn(move || http_get(&format!("http://127.0.0.1:{port}/?error=access_denied&state={state}")));
        assert!(matches!(r.wait_for_code(&cancel, Duration::from_secs(10)), Err(OAuthError::Denied(t)) if t.contains("cancelled")));
    }

    // spec: MAIL-084, MAIL-082, SEC-017
    #[test]
    fn sasl_responses_follow_the_specifications() {
        assert_eq!(*xoauth2_response("someuser@example.com", "ya29.vF9dft4qmTc2Nvb3RlckBhbHRhdmlzdGEuY29tCg"), b"user=someuser@example.com\x01auth=Bearer ya29.vF9dft4qmTc2Nvb3RlckBhbHRhdmlzdGEuY29tCg\x01\x01".to_vec());
        // Google's documented example, base64
        assert_eq!(
            base64_encode(&xoauth2_response("someuser@example.com", "ya29.vF9dft4qmTc2Nvb3RlckBhbHRhdmlzdGEuY29tCg")),
            "dXNlcj1zb21ldXNlckBleGFtcGxlLmNvbQFhdXRoPUJlYXJlciB5YTI5LnZGOWRmdDRxbVRjMk52YjNSbGNrQmhiSFJoZG1semRHRXVZMjl0Q2cBAQ=="
        );
        // RFC 7628 §4.1
        assert_eq!(
            *oauthbearer_response("user@example.com", "server.example.com", 143, "vF9dft4qmTc2Nvb3RlckBhbHRhdmlzdGEuY29tCg=="),
            b"n,a=user@example.com,\x01host=server.example.com\x01port=143\x01auth=Bearer vF9dft4qmTc2Nvb3RlckBhbHRhdmlzdGEuY29tCg==\x01\x01".to_vec()
        );
        assert_eq!(*oauthbearer_response("a=b,c", "h", 1, "t"), b"n,a=a=3Db=2Cc,\x01host=h\x01port=1\x01auth=Bearer t\x01\x01".to_vec());
        // OAUTHBEARER is preferred when offered; otherwise XOAUTH2
        let mut a = OAuthAuth::new("u", "TOKVALUE", "h", 993);
        a.select_mechanism(&|m| m == "XOAUTH2");
        assert_eq!(a.mechanism(), "XOAUTH2");
        assert_eq!(a.respond(b"{\"status\":\"401\"}"), Vec::<u8>::new(), "XOAUTH2: an empty line after the error");
        a.select_mechanism(&|m| m == "XOAUTH2" || m == "OAUTHBEARER");
        assert_eq!(a.mechanism(), "OAUTHBEARER");
        assert_eq!(a.respond(b"{}"), vec![1u8], "OAUTHBEARER: a single ^A after the error");
        a.select_mechanism(&|_| false);
        assert_eq!(a.mechanism(), "XOAUTH2");
        // no token in debug output
        let d = format!("{a:?} {:?}", Tokens { access_token: "SECRET-AT".into(), refresh_token: Some("SECRET-RT".into()), expires_in: 1, user: None });
        assert!(!d.contains("TOKVALUE") && !d.contains("SECRET"), "{d}");
        let cfg = OAuthConfig::for_account(&MailAccount { oauth_client_id: "id".into(), ..MailAccount::default() }, Some("SECRET-CS".into())).unwrap();
        assert!(!format!("{cfg:?}").contains("SECRET-CS"));
    }

    // spec: MAIL-082, SEC-016
    #[test]
    fn token_answers_are_read_and_errors_are_told_apart() {
        let t = parse_token_response(200, r#"{"access_token":"at","token_type":"Bearer","expires_in":3599,"refresh_token":"rt","scope":"mail"}"#, false).unwrap();
        assert_eq!((t.access_token.expose(), t.refresh_token.as_ref().map(|s| s.expose()), t.expires_in, t.user), ("at", Some("rt"), 3599, None));
        // string expiry, no refresh token, lower-case type
        let t = parse_token_response(200, r#"{"access_token":"at","token_type":"bearer","expires_in":"120"}"#, true).unwrap();
        assert_eq!((t.refresh_token, t.expires_in), (None, 120));
        // the user from an ID token (payload {"email":"ada@example.com"})
        let payload = base64url(br#"{"email":"ada@example.com","sub":"1"}"#);
        let t = parse_token_response(200, &format!(r#"{{"access_token":"at","id_token":"x.{payload}.y"}}"#), false).unwrap();
        assert_eq!(t.user.as_deref(), Some("ada@example.com"));
        assert_eq!(parse_token_response(400, r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#, true), Err(OAuthError::Revoked));
        assert!(matches!(parse_token_response(400, r#"{"error":"invalid_grant"}"#, false), Err(OAuthError::Rejected(t)) if t.contains("sign-in code")));
        assert!(matches!(parse_token_response(401, r#"{"error":"invalid_client"}"#, false), Err(OAuthError::Rejected(t)) if t.contains("client ID")));
        assert!(matches!(parse_token_response(200, r#"{"token_type":"Bearer"}"#, false), Err(OAuthError::Rejected(_))));
        assert!(matches!(parse_token_response(200, r#"{"access_token":"a","token_type":"mac"}"#, false), Err(OAuthError::Rejected(_))));
        assert!(matches!(parse_token_response(500, "<html>oops</html>", false), Err(OAuthError::Network(_))));
        assert!(matches!(parse_token_response(502, "{}", false), Err(OAuthError::Rejected(_))));
        for junk in ["", "[]", "null", "{\"access_token\":5}", "\u{0}"] {
            assert!(parse_token_response(200, junk, false).is_err(), "{junk}");
        }
        // a form body is percent-encoded
        assert_eq!(form_body(&[("a b", "c&d=e"), ("x", "ü")]).as_str(), "a%20b=c%26d%3De&x=%C3%BC");
    }

    // spec: MAIL-080, MAIL-082
    #[test]
    fn a_whole_sign_in_against_the_pretend_provider_keeps_the_refresh_token_and_checks_pkce() {
        if !has_curl() {
            return;
        }
        let f = FakeOAuth::start();
        let account = custom(&f);
        let f2 = f.state.clone();
        let port_of = f.port;
        // the pretend browser: asks the provider's sign-in page, then follows its redirect
        let _b = crate::opener::install_test_browser(Box::new(move |url| {
            let url = url.to_string();
            std::thread::spawn(move || {
                let (status, headers, _) = http_get(&url);
                assert_eq!(status, 302, "the provider redirects back");
                let loc = headers.lines().find_map(|l| l.strip_prefix("Location: ").or_else(|| l.strip_prefix("location: "))).expect("Location header").trim().to_string();
                assert!(loc.starts_with("http://127.0.0.1:"), "{loc}");
                let _ = http_get(&loc);
            });
            let _ = port_of;
        }));
        let cancel = AtomicBool::new(false);
        let shown = Mutex::new(String::new());
        let r = sign_in(&account, None, &cancel, Duration::from_secs(20), &|u| *shown.lock().unwrap() = u.to_string()).unwrap();
        assert_eq!(r.user.as_deref(), Some("ada@example.com"), "the ID token says who signed in");
        assert!(shown.lock().unwrap().contains("code_challenge="));
        let st = f2.lock().unwrap();
        assert_eq!(st.token_requests.len(), 1);
        let req = &st.token_requests[0];
        let get = |k: &str| req.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
        assert_eq!(get("grant_type"), "authorization_code");
        assert_eq!(get("client_id"), "shale-test");
        assert!(get("code_verifier").len() >= 43);
        assert!(get("redirect_uri").starts_with("http://127.0.0.1:"));
        drop(st);
        // the keyring (the pretend one in tests) has the refresh token; the access token is only in memory
        assert!(secrets::mail_secret("oauth-t", MailSecret::RefreshToken).is_some());
        assert!(is_signed_in(&account));
        secrets::clear_mail_secret("oauth-t", MailSecret::RefreshToken);
        forget_source("oauth-t");
    }

    // spec: MAIL-082
    #[test]
    fn a_wrong_pkce_verifier_or_client_is_refused_by_the_provider() {
        if !has_curl() {
            return;
        }
        let f = FakeOAuth::start();
        let cfg = cfg_for(&f);
        let code = f.issue_code(&pkce_challenge("the-real-verifier"), "http://127.0.0.1:1/");
        assert!(matches!(exchange_code(&cfg, &code, "http://127.0.0.1:1/", "another-verifier"), Err(OAuthError::Rejected(_))));
        // a code works once
        let code = f.issue_code(&pkce_challenge("v1"), "http://127.0.0.1:1/");
        assert!(exchange_code(&cfg, &code, "http://127.0.0.1:1/", "v1").is_ok());
        assert!(exchange_code(&cfg, &code, "http://127.0.0.1:1/", "v1").is_err());
        let mut bad = cfg.clone();
        bad.client_id = "somebody-else".into();
        let code = f.issue_code(&pkce_challenge("v2"), "http://127.0.0.1:1/");
        assert!(matches!(exchange_code(&bad, &code, "http://127.0.0.1:1/", "v2"), Err(OAuthError::Rejected(t)) if t.contains("client ID")));
        // the client secret goes along when there is one
        f.state.lock().unwrap().client_secret = Some("s3cret".into());
        let code = f.issue_code(&pkce_challenge("v3"), "http://127.0.0.1:1/");
        assert!(exchange_code(&cfg, &code, "http://127.0.0.1:1/", "v3").is_err(), "the provider wants a secret");
        let with = OAuthConfig { client_secret: Some("s3cret".into()), ..cfg };
        let code = f.issue_code(&pkce_challenge("v4"), "http://127.0.0.1:1/");
        assert!(exchange_code(&with, &code, "http://127.0.0.1:1/", "v4").is_ok());
    }

    // spec: MAIL-083
    #[test]
    fn the_access_token_is_renewed_when_it_expires_or_is_refused_and_a_revoked_token_says_so() {
        if !has_curl() {
            return;
        }
        let f = FakeOAuth::start();
        let cfg = cfg_for(&f);
        let rt = f.new_refresh_token();
        let store = Arc::new(MemStore(Mutex::new(Some(Secret::new(&rt)))));
        let src = TokenSource::new(cfg.clone(), store.clone());
        // first use: refresh; second: the one in memory
        let a1 = src.access_token().unwrap();
        assert_eq!(src.access_token().unwrap(), a1);
        assert_eq!(src.refresh_count(), 1, "a token with time left is reused");
        // the server refused it: the next call gets a new one
        src.reject();
        let a2 = src.access_token().unwrap();
        assert_ne!(a1, a2);
        assert_eq!(src.refresh_count(), 2);
        // a token that expires within the margin is renewed
        f.state.lock().unwrap().expires_in = 30;
        src.reject();
        let a3 = src.access_token().unwrap();
        assert_ne!(a2, a3);
        let a4 = src.access_token().unwrap();
        assert_ne!(a3, a4, "30 seconds left is less than the margin");
        f.state.lock().unwrap().expires_in = 3600;
        // the provider rotates the refresh token: the new one is kept
        f.state.lock().unwrap().rotate = true;
        src.reject();
        src.access_token().unwrap();
        assert_ne!(store.load().unwrap(), rt, "the rotated refresh token was saved");
        // revoked
        f.state.lock().unwrap().revoked = true;
        src.reject();
        let e = src.access_token().unwrap_err();
        assert_eq!(e, OAuthError::Revoked);
        assert!(needs_sign_in(&e.to_string()));
        assert_eq!(store.load(), None, "a dead refresh token is not kept");
        assert_eq!(src.access_token().unwrap_err(), OAuthError::NotSignedIn);
        // never signed in
        let none = TokenSource::new(cfg, Arc::new(MemStore(Mutex::new(None))));
        assert_eq!(none.access_token(), Err(OAuthError::NotSignedIn));
        assert!(needs_sign_in(&OAuthError::NotSignedIn.to_string()));
    }

    // spec: MAIL-082
    #[test]
    fn the_token_endpoint_is_not_followed_to_another_address_and_an_unreachable_one_is_an_error() {
        if !has_curl() {
            return;
        }
        let f = FakeOAuth::start();
        let mut cfg = cfg_for(&f);
        f.state.lock().unwrap().redirect_token_to = Some("http://127.0.0.1:1/elsewhere".into());
        let r = refresh(&cfg, "whatever");
        assert!(r.is_err(), "a redirect from the token endpoint is not followed: {r:?}");
        assert_eq!(f.state.lock().unwrap().token_requests.len(), 1);
        // nothing listens there
        cfg.token_url = "http://127.0.0.1:1/token".into();
        assert!(matches!(refresh(&cfg, "x"), Err(OAuthError::Network(_))));
        // a non-https address from outside the test environment is refused before curl runs
        cfg.token_url = "http://example.com/token".into();
        assert!(matches!(refresh(&cfg, "x"), Err(OAuthError::Config(_))));
    }
}

/// The whole `sign_in` (the real listener, the real token exchange, the keyring of the tests)
/// against the pretend provider, with the pretend browser deciding what the person does.
#[cfg(test)]
mod pretend_browser_tests {
    use super::*;
    use crate::mail::oauth_fake_tests::{pretend_browser, FakeOAuth, Visit};

    fn has_curl() -> bool {
        std::process::Command::new("curl").arg("--version").output().is_ok()
    }

    /// An account of the pretend provider with an id of its own (keyring and registry are shared by tests).
    fn account(id: &str, f: &FakeOAuth) -> MailAccount {
        MailAccount {
            id: id.into(),
            user: "ada@example.com".into(),
            oauth_provider: OAuthProvider::Custom,
            oauth_client_id: "shale-test".into(),
            oauth_auth_url: format!("http://127.0.0.1:{}/authorize", f.port),
            oauth_token_url: format!("http://127.0.0.1:{}/token", f.port),
            oauth_scope: "mail".into(),
            ..MailAccount::default()
        }
    }

    fn forget(id: &str) {
        secrets::clear_mail_secret(id, MailSecret::RefreshToken);
        forget_source(id);
    }

    // spec: MAIL-080, MAIL-083
    #[test]
    fn the_pretend_browser_approves_and_the_sign_in_keeps_the_refresh_token_in_the_keyring() {
        if !has_curl() {
            return;
        }
        let f = FakeOAuth::start();
        let a = account("oauth-pb-ok", &f);
        let (_guard, log) = pretend_browser(Visit::Approve);
        let shown = Mutex::new(Vec::<String>::new());
        let r = sign_in(&a, None, &AtomicBool::new(false), Duration::from_secs(20), &|u| shown.lock().unwrap().push(u.to_string())).unwrap();
        assert_eq!(r.user.as_deref(), Some("ada@example.com"));
        assert!(r.kept, "the keyring took the refresh token");
        assert_eq!(log.wait_callbacks(1), vec![200], "the callback said the sign-in worked");
        assert_eq!(*log.problems.lock().unwrap(), Vec::<String>::new(), "the address the app gave the browser was right");
        // the address that was shown is the one that was opened, once
        assert_eq!(*shown.lock().unwrap(), *log.opened.lock().unwrap());
        assert_eq!(log.opened.lock().unwrap().len(), 1);
        // the provider saw one sign-in page and one code exchange with the matching PKCE verifier
        let st = f.state.lock().unwrap();
        assert_eq!((st.authorize_requests.len(), st.token_requests.len()), (1, 1));
        drop(st);
        assert!(secrets::mail_secret("oauth-pb-ok", MailSecret::RefreshToken).is_some());
        assert!(is_signed_in(&a));
        forget("oauth-pb-ok");
    }

    // spec: MAIL-081
    #[test]
    fn a_no_in_the_browser_ends_the_sign_in_with_a_message_and_keeps_nothing() {
        let f = FakeOAuth::start();
        let a = account("oauth-pb-denied", &f);
        forget(&a.id);
        // the person says no on the callback itself ...
        let (guard, log) = pretend_browser(Visit::Deny);
        let r = sign_in(&a, None, &AtomicBool::new(false), Duration::from_secs(20), &|_| {});
        assert!(matches!(&r, Err(OAuthError::Denied(t)) if t.contains("cancelled")), "{r:?}");
        assert_eq!(log.wait_callbacks(1), vec![200], "the browser page tells the person that it is over");
        assert!(log.problems.lock().unwrap().is_empty());
        drop(guard);
        // ... or the provider's own sign-in page redirects with the error
        f.state.lock().unwrap().deny = true;
        let (_guard, log) = pretend_browser(Visit::Approve);
        let r = sign_in(&a, None, &AtomicBool::new(false), Duration::from_secs(20), &|_| {});
        assert!(matches!(&r, Err(OAuthError::Denied(_))), "{r:?}");
        assert!(log.problems.lock().unwrap().is_empty());
        let st = f.state.lock().unwrap();
        assert!(st.token_requests.is_empty(), "no code was exchanged: {:?}", st.token_requests);
        drop(st);
        assert!(secrets::mail_secret(&a.id, MailSecret::RefreshToken).is_none());
        assert!(!is_signed_in(&a));
    }

    // spec: MAIL-081
    #[test]
    fn an_answer_with_the_wrong_state_is_refused_and_the_real_answer_still_signs_in() {
        if !has_curl() {
            return;
        }
        let f = FakeOAuth::start();
        let a = account("oauth-pb-state", &f);
        forget(&a.id);
        let (_guard, log) = pretend_browser(Visit::WrongStateThenApprove);
        let r = sign_in(&a, None, &AtomicBool::new(false), Duration::from_secs(20), &|_| {});
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(log.wait_callbacks(2), vec![400, 200], "the wrong state got an error page, the right one the success page");
        assert!(log.problems.lock().unwrap().is_empty());
        assert_eq!(f.state.lock().unwrap().token_requests.len(), 1, "only the real code was exchanged, not `evil`");
        forget(&a.id);
    }

    // spec: MAIL-081, JOB-002
    #[test]
    fn a_wrong_state_alone_does_not_sign_in_and_cancel_ends_the_job() {
        let f = FakeOAuth::start();
        let a = account("oauth-pb-cancel", &f);
        forget(&a.id);
        let (_guard, log) = pretend_browser(Visit::WrongStateOnly);
        let cancel = Arc::new(AtomicBool::new(false));
        let (c2, a2) = (cancel.clone(), a.clone());
        // the sign-in runs as a job, the way the account settings run it
        let rx = crate::jobs::spawn(move || sign_in(&a2, None, &c2, Duration::from_secs(60), &|_| {}), || {});
        assert_eq!(log.wait_callbacks(1), vec![400], "the wrong state is refused");
        assert!(rx.try_recv().is_err(), "and the sign-in keeps waiting");
        cancel.store(true, Ordering::SeqCst);
        let r = rx.recv_timeout(Duration::from_secs(10)).expect("the job ended (the spinner stops)");
        assert_eq!(r.unwrap().err(), Some(OAuthError::Cancelled));
        assert!(f.state.lock().unwrap().token_requests.is_empty());
        assert!(secrets::mail_secret(&a.id, MailSecret::RefreshToken).is_none());
    }

    // spec: MAIL-081, JOB-002
    #[test]
    fn a_browser_that_never_answers_ends_with_a_time_out_and_cancel_works_without_any_callback() {
        let f = FakeOAuth::start();
        let a = account("oauth-pb-silent", &f);
        forget(&a.id);
        let (_guard, log) = pretend_browser(Visit::Silent);
        // time-out
        let (a2, f_port) = (a.clone(), f.port);
        let rx = crate::jobs::spawn(move || sign_in(&a2, None, &AtomicBool::new(false), Duration::from_millis(300), &|_| {}), || {});
        let r = rx.recv_timeout(Duration::from_secs(10)).expect("the job ended (the spinner stops)");
        assert_eq!(r.unwrap().err(), Some(OAuthError::TimedOut));
        assert_eq!(log.opened.lock().unwrap().len(), 1);
        // cancel, as soon as the browser was opened
        let cancel = Arc::new(AtomicBool::new(false));
        let (c2, a3) = (cancel.clone(), a.clone());
        let rx = crate::jobs::spawn(move || sign_in(&a3, None, &c2, Duration::from_secs(60), &|_| {}), || {});
        log.wait_opened(2);
        cancel.store(true, Ordering::SeqCst);
        let r = rx.recv_timeout(Duration::from_secs(10)).expect("the job ended (the spinner stops)");
        assert_eq!(r.unwrap().err(), Some(OAuthError::Cancelled));
        // the browser was given a good address both times and nothing was kept
        assert!(log.problems.lock().unwrap().is_empty(), "{:?}", log.problems.lock().unwrap());
        assert!(log.opened.lock().unwrap().iter().all(|u| u.starts_with(&format!("http://127.0.0.1:{f_port}/authorize?"))));
        assert!(log.callbacks.lock().unwrap().is_empty());
        assert!(!is_signed_in(&a));
    }

    /// The sockets of this process that listen on 127.0.0.1:`port` (their inode numbers), from `/proc` on Linux; empty elsewhere.
    fn listening_sockets(port: u16) -> Vec<u64> {
        let Ok(table) = std::fs::read_to_string("/proc/net/tcp") else { return Vec::new() };
        let local = format!("0100007F:{port:04X}");
        table
            .lines()
            .skip(1)
            .filter_map(|l| {
                let f: Vec<&str> = l.split_whitespace().collect();
                (f.get(1) == Some(&local.as_str()) && f.get(3) == Some(&"0A")).then(|| f.get(9).and_then(|i| i.parse().ok())).flatten()
            })
            .collect()
    }

    // spec: JOB-032
    #[test]
    fn job_032_cancel_frees_the_redirect_listener_and_a_token_that_arrives_with_cancel_is_not_kept() {
        if !has_curl() {
            return;
        }
        let f = FakeOAuth::start();
        let a = account("oauth-job032", &f);
        forget(&a.id);
        // Cancel while waiting for the browser: the redirect port is free again when the sign-in returns
        let (guard, log) = pretend_browser(Visit::Silent);
        let url = Arc::new(Mutex::new(String::new()));
        let cancel = Arc::new(AtomicBool::new(false));
        let (u2, c2, a2) = (url.clone(), cancel.clone(), a.clone());
        let rx = crate::jobs::spawn(move || sign_in(&a2, None, &c2, Duration::from_secs(60), &|u| *u2.lock().unwrap() = u.to_string()), || {});
        log.wait_opened(1);
        let port: u16 = url.lock().unwrap().split("redirect_uri=http%3A%2F%2F127.0.0.1%3A").nth(1).and_then(|r| r.split("%2F").next()).and_then(|p| p.parse().ok()).expect("the redirect port is in the address");
        assert!(TcpListener::bind(("127.0.0.1", port)).is_err(), "the listener is bound while it waits");
        let mine = listening_sockets(port);
        cancel.store(true, Ordering::SeqCst);
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)).expect("the job ended").unwrap().err(), Some(OAuthError::Cancelled));
        // The port is an ephemeral one, and the other tests of this binary (threads of one process) bind port 0 all the time:
        // one of them can be given this very port the moment Shale lets go of it, and keep it for its whole run (#659). So
        // "bind fails" does not say that Shale still listens. `sign_in` drops its listener before it returns, i.e. before
        // the job's result is sent; what is checked is that Shale's own socket is gone (a later holder of the port is
        // another socket). Even then the socket can outlive the drop for a moment: the other tests of this binary spawn
        // child processes (curl, the pretend browser), and a child forked in that instant holds a copy of every open fd
        // (CLOEXEC only closes it at its `exec`), which under heavy load can take a while (#659). So the check is
        // retried for a bounded time. Where the sockets cannot be listed (not Linux) `mine` is empty and the port is bound
        // again instead, which can only fail when another test took it first.
        assert!(mine.len() <= 1, "only Shale's listener holds the port while it waits: {mine:?}");
        let freed = (0..400).any(|_| {
            let gone = match mine.first() {
                Some(inode) => !listening_sockets(port).contains(inode),
                None => TcpListener::bind(("127.0.0.1", port)).is_ok(),
            };
            gone || {
                std::thread::sleep(Duration::from_millis(25));
                false
            }
        });
        assert!(freed, "Cancel freed the redirect port");
        drop(guard);
        // Cancel that comes while the code is traded for tokens: the tokens are dropped, nothing is kept, not even in memory
        let (guard, log) = pretend_browser(Visit::Approve);
        let cancel = Arc::new(AtomicBool::new(false));
        f.state.lock().unwrap().cancel_on_exchange = Some(cancel.clone());
        let r = sign_in(&a, None, &cancel, Duration::from_secs(20), &|_| {});
        assert_eq!(r.err(), Some(OAuthError::Cancelled));
        assert_eq!(log.wait_callbacks(1), vec![200]);
        assert_eq!(f.state.lock().unwrap().token_requests.len(), 1, "the code was traded: the token did arrive");
        assert!(secrets::mail_secret(&a.id, MailSecret::RefreshToken).is_none(), "no token in the keyring");
        assert!(!is_signed_in(&a), "and none in memory");
        // without Cancel the same flow keeps the whole sign-in
        f.state.lock().unwrap().cancel_on_exchange = None;
        drop(guard);
        let (_guard, _log) = pretend_browser(Visit::Approve);
        let r = sign_in(&a, None, &AtomicBool::new(false), Duration::from_secs(20), &|_| {}).unwrap();
        assert!(r.kept && is_signed_in(&a));
        forget(&a.id);
    }
}
