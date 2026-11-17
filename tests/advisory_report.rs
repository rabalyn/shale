//! `scripts/report-advisories.sh` (BUILD-021) against a pretend GitHub that keeps its issues and
//! labels in memory, so a second run of the script sees what the first one wrote. The input is a
//! file in the shape of `cargo audit --json`. The script needs `sh`, `curl` and `jq`; a missing one
//! fails the test with a message (no silent skip).
#![cfg(unix)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{json, Value};

const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/report-advisories.sh");
const TOKEN: &str = "secret-token";

// ---- what cargo audit says

fn advisory(id: &str, name: &str, version: &str) -> Value {
    json!({
        "advisory": {"id": id, "package": name, "title": format!("Bad thing in {name}"), "description": "Long text.",
                     "date": "2026-11-01", "url": format!("https://rustsec.org/advisories/{id}"), "aliases": ["CVE-2026-1"]},
        "versions": {"patched": [">=9.9.9"], "unaffected": []},
        "package": {"name": name, "version": version},
    })
}

fn warning(kind: &str, id: Option<&str>, name: &str, version: &str) -> Value {
    let adv = id.map(|id| json!({"id": id, "package": name, "title": format!("{name} is {kind}"), "url": null}));
    json!({"kind": kind, "package": {"name": name, "version": version}, "advisory": adv, "versions": null})
}

fn audit(vulns: Vec<Value>, warnings: Vec<Value>) -> Value {
    let mut w = serde_json::Map::new();
    for x in warnings {
        let kind = x["kind"].as_str().unwrap().to_string();
        w.entry(kind).or_insert_with(|| json!([])).as_array_mut().unwrap().push(x);
    }
    json!({"database": {}, "lockfile": {"dependency-count": 1}, "settings": {},
           "vulnerabilities": {"found": !vulns.is_empty(), "count": vulns.len(), "list": vulns}, "warnings": w})
}

// ---- what GitHub knows

#[derive(Clone)]
struct Issue {
    number: u64,
    title: String,
    body: String,
    labels: Vec<String>,
    state: &'static str,
    reason: Option<String>,
    comments: Vec<String>,
}

fn issue_json(i: &Issue) -> Value {
    let labels: Vec<Value> = i.labels.iter().map(|l| json!({"name": l})).collect();
    json!({"number": i.number, "title": i.title, "body": i.body, "state": i.state, "state_reason": i.reason, "labels": labels})
}

fn param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
}

type Reply = Option<(u16, String)>;

fn reply(code: u16, v: Value) -> Reply {
    Some((code, v.to_string()))
}

#[derive(Clone, Copy)]
enum Mode {
    Normal,
    Status(u16),
    HangUp,
}

struct State {
    mode: Mode,
    issues: Vec<Issue>,
    labels: Vec<String>,
    log: Vec<String>,
}

impl State {
    fn handle(&mut self, method: &str, target: &str, auth: Option<&str>, body: &str) -> Reply {
        self.log.push(format!("{method} {target}"));
        match self.mode {
            Mode::HangUp => return None,
            Mode::Status(code) => return reply(code, json!({"message": "no"})),
            Mode::Normal => {}
        }
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let Some(p) = path.strip_prefix("/gh/repos/rabalyn/shale/") else {
            return reply(404, json!({"message": "Not Found"}));
        };
        if auth != Some(format!("Bearer {TOKEN}").as_str()) {
            return reply(401, json!({"message": "Bad credentials"}));
        }
        let sent: Value = serde_json::from_str(body).unwrap_or_default();
        let text = |key: &str| sent[key].as_str().unwrap_or_default().to_string();
        let parts: Vec<&str> = p.split('/').collect();
        match (method, parts.as_slice()) {
            ("GET", ["labels", name]) => {
                let name = name.replace("%2F", "/");
                return if self.labels.contains(&name) { reply(200, json!({"name": name})) } else { reply(404, json!({"message": "Not Found"})) };
            }
            ("POST", ["labels"]) => {
                self.labels.push(text("name"));
                return reply(201, json!({"name": text("name")}));
            }
            ("GET", ["issues"]) => {
                let label = param(query, "labels").unwrap_or_default();
                let per: usize = param(query, "per_page").and_then(|v| v.parse().ok()).unwrap_or(30);
                let page: usize = param(query, "page").and_then(|v| v.parse().ok()).unwrap_or(1);
                assert_eq!(param(query, "state"), Some("all"));
                let all: Vec<Value> = self.issues.iter().filter(|i| label.is_empty() || i.labels.iter().any(|l| l == label)).skip((page - 1) * per).take(per).map(issue_json).collect();
                return reply(200, Value::Array(all));
            }
            ("POST", ["issues"]) => {
                let labels: Vec<String> = sent["labels"].as_array().unwrap().iter().map(|l| l.as_str().unwrap().to_string()).collect();
                for l in &labels {
                    assert!(self.labels.contains(l), "label {l} was used before it was made");
                }
                let number = self.issues.len() as u64 + 1;
                let issue = Issue { number, title: text("title"), body: text("body"), labels, state: "open", reason: None, comments: vec![] };
                let created = issue_json(&issue);
                self.issues.push(issue);
                return reply(201, created);
            }
            _ => {}
        }
        let number = parts.get(1).and_then(|n| n.parse::<u64>().ok());
        let Some(issue) = self.issues.iter_mut().find(|i| Some(i.number) == number) else {
            return reply(404, json!({"message": "Not Found"}));
        };
        match (method, parts.as_slice()) {
            ("PATCH", ["issues", _]) => {
                if text("state") == "closed" {
                    issue.state = "closed";
                    issue.reason = Some(text("state_reason"));
                } else {
                    issue.state = "open";
                    issue.reason = Some("reopened".into());
                }
                reply(200, issue_json(issue))
            }
            ("POST", ["issues", _, "comments"]) => {
                issue.comments.push(text("body"));
                reply(201, json!({"body": text("body")}))
            }
            _ => reply(404, json!({"message": "no such route"})),
        }
    }
}

// ---- the pretend server

struct Fake {
    addr: SocketAddr,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    input: PathBuf,
}

fn read_request(s: &mut TcpStream) -> Option<(String, String, Option<String>, String)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let n = s.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next()?.split(' ');
    let (method, target) = (first.next()?.to_string(), first.next()?.to_string());
    let (mut auth, mut len, mut expect) = (None, 0usize, false);
    for line in lines {
        let (key, value) = line.split_once(':')?;
        match key.to_ascii_lowercase().as_str() {
            "authorization" => auth = Some(value.trim().to_string()),
            "content-length" => len = value.trim().parse().ok()?,
            "expect" => expect = true,
            _ => {}
        }
    }
    if expect {
        s.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").ok()?;
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = s.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Some((method, target, auth, String::from_utf8_lossy(&body).to_string()))
}

fn serve(listener: TcpListener, state: Arc<Mutex<State>>, stop: Arc<AtomicBool>) {
    for conn in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(mut stream) = conn else { continue };
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let Some((method, target, auth, body)) = read_request(&mut stream) else { continue };
        let answer = state.lock().unwrap().handle(&method, &target, auth.as_deref(), &body);
        // no answer: the connection is closed at once
        if let Some((code, text)) = answer {
            let head = format!("HTTP/1.1 {code} Reply\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", text.len());
            let _ = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(text.as_bytes()));
        }
    }
}

struct Ran {
    ok: bool,
    out: String,
}

fn need(tool: &str) {
    assert!(Command::new(tool).arg("--version").output().is_ok(), "`{tool}` is not installed: scripts/report-advisories.sh and this test (BUILD-021) need it");
}

impl Fake {
    /// A GitHub that already has an issue of a person about security, without a marker, which
    /// the script must never touch.
    fn start() -> Fake {
        for tool in ["sh", "curl", "jq"] {
            need(tool);
        }
        let own = Issue { number: 1, title: "Is the vault encrypted?".into(), body: "A question.".into(), labels: vec!["security".into()], state: "open", reason: None, comments: vec![] };
        let state = Arc::new(Mutex::new(State { mode: Mode::Normal, issues: vec![own], labels: vec!["security".into()], log: vec![] }));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::spawn({
            let (state, stop) = (state.clone(), stop.clone());
            move || serve(listener, state, stop)
        });
        let input = std::env::temp_dir().join(format!("shale-advisory-report-{}-{}.json", std::process::id(), addr.port()));
        Fake { addr, state, stop, thread: Some(thread), input }
    }

    fn mode(&self, mode: Mode) {
        self.state.lock().unwrap().mode = mode;
    }

    fn issues(&self) -> Vec<Issue> {
        self.state.lock().unwrap().issues.clone()
    }

    /// The issues made by the script.
    fn reports(&self) -> Vec<Issue> {
        self.issues().into_iter().filter(|i| i.body.contains("<!-- shale-advisory:")).collect()
    }

    fn report(&self, key: &str) -> Issue {
        let marker = format!("<!-- shale-advisory:{key} -->");
        let found: Vec<Issue> = self.reports().into_iter().filter(|i| i.body.contains(&marker)).collect();
        assert_eq!(found.len(), 1, "one issue for {key}");
        found[0].clone()
    }

    fn set_reason(&self, number: u64, state: &'static str, reason: &str) {
        let mut s = self.state.lock().unwrap();
        let i = s.issues.iter_mut().find(|i| i.number == number).unwrap();
        i.state = state;
        i.reason = Some(reason.into());
    }

    /// Requests that change something (not GET).
    fn writes(&self) -> usize {
        self.state.lock().unwrap().log.iter().filter(|l| !l.starts_with("GET ")).count()
    }

    fn input(&self, audit: &Value) -> PathBuf {
        std::fs::write(&self.input, audit.to_string()).unwrap();
        self.input.clone()
    }

    fn script_with(&self, audit: &Value, tweak: impl FnOnce(&mut Command)) -> Ran {
        let mut c = Command::new("sh");
        c.arg(SCRIPT).arg(self.input(audit));
        for var in ["GITHUB_API", "GITHUB_REPO", "GH_TOKEN", "AUDIT_COMMIT", "http_proxy", "https_proxy", "all_proxy", "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
            c.env_remove(var);
        }
        c.env("NO_PROXY", "*").env("no_proxy", "*");
        c.env("GITHUB_API", format!("http://{}/gh", self.addr)).env("GH_TOKEN", TOKEN).env("AUDIT_COMMIT", "abc1234");
        tweak(&mut c);
        let output = c.output().expect("sh starts");
        let out = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        Ran { ok: output.status.success(), out }
    }

    /// The script runs on `audit` and succeeds.
    fn pass(&self, audit: &Value) -> String {
        let ran = self.script_with(audit, |_| {});
        assert!(ran.ok, "the script failed:\n{}", ran.out);
        assert!(!ran.out.contains(TOKEN), "the token was printed:\n{}", ran.out);
        ran.out
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let _ = std::fs::remove_file(&self.input);
    }
}

/// The script fails with an `::error::` line that contains `needle`.
fn assert_error(ran: &Ran, needle: &str) {
    assert!(!ran.ok, "the script should fail:\n{}", ran.out);
    let line = ran.out.lines().find(|l| l.starts_with("::error::"));
    assert!(line.is_some_and(|l| l.contains(needle)), "no ::error:: line with {needle:?}:\n{}", ran.out);
    assert!(!ran.out.contains(TOKEN), "the token was printed:\n{}", ran.out);
}

fn one_vuln() -> Value {
    audit(vec![advisory("RUSTSEC-2026-0001", "ring", "0.17.1")], vec![])
}

// ---- the tests

// spec: BUILD-021
#[test]
fn build_021_a_vulnerability_opens_one_issue_with_what_to_do() {
    let fake = Fake::start();
    let out = fake.pass(&one_vuln());
    assert!(out.contains("opened #2"), "{out}");
    let issue = fake.report("RUSTSEC-2026-0001");
    assert_eq!(issue.title, "Security: RUSTSEC-2026-0001 in ring 0.17.1: Bad thing in ring");
    assert_eq!(issue.labels, ["security", "security/vulnerability"]);
    for wanted in ["(vulnerability) at abc1234", "`ring 0.17.1`", "https://rustsec.org/advisories/RUSTSEC-2026-0001", "Fixed in: >=9.9.9", "CVE-2026-1", "Long text.", "*not planned*"] {
        assert!(issue.body.contains(wanted), "{wanted:?} is missing in:\n{}", issue.body);
    }
    assert!(fake.state.lock().unwrap().labels.contains(&"security/vulnerability".to_string()), "the kind label was made");
}

// spec: BUILD-021
#[test]
fn build_021_a_reported_finding_is_not_reported_again() {
    let fake = Fake::start();
    fake.pass(&one_vuln());
    let writes = fake.writes();
    let out = fake.pass(&one_vuln());
    assert!(out.contains("nothing new (1 findings, all reported)"), "{out}");
    assert_eq!(fake.writes(), writes, "the second pass wrote something");
    assert_eq!(fake.reports().len(), 1);
}

// spec: BUILD-021
#[test]
fn build_021_one_advisory_in_two_versions_is_one_issue() {
    let fake = Fake::start();
    fake.pass(&audit(vec![advisory("RUSTSEC-2026-0002", "idna", "0.5.0"), advisory("RUSTSEC-2026-0002", "idna", "1.0.0")], vec![]));
    let issue = fake.report("RUSTSEC-2026-0002");
    assert!(issue.title.contains("idna 0.5.0, idna 1.0.0"), "{}", issue.title);
}

// spec: BUILD-021
#[test]
fn build_021_warnings_are_reported_with_their_kind() {
    let fake = Fake::start();
    let warnings = vec![warning("unmaintained", Some("RUSTSEC-2026-0003"), "paste", "1.0.15"), warning("unsound", Some("RUSTSEC-2026-0004"), "atty", "0.2.14"), warning("yanked", None, "foo", "1.2.3")];
    fake.pass(&audit(vec![], warnings));
    assert_eq!(fake.report("RUSTSEC-2026-0003").labels, ["security", "security/unmaintained"]);
    assert_eq!(fake.report("RUSTSEC-2026-0004").labels, ["security", "security/unsound"]);
    let yanked = fake.report("yanked:foo@1.2.3");
    assert_eq!(yanked.labels, ["security", "security/yanked"]);
    assert_eq!(yanked.title, "Security: yanked:foo@1.2.3 in foo 1.2.3");
}

// spec: BUILD-021
#[test]
fn build_021_a_finding_that_is_gone_closes_its_issue() {
    let fake = Fake::start();
    fake.pass(&one_vuln());
    let out = fake.pass(&audit(vec![], vec![]));
    assert!(out.contains("closed #2"), "{out}");
    let issue = fake.report("RUSTSEC-2026-0001");
    assert_eq!((issue.state, issue.reason.as_deref()), ("closed", Some("completed")));
    assert_eq!(issue.comments, ["cargo audit no longer reports RUSTSEC-2026-0001 at abc1234."]);
    let own = &fake.issues()[0];
    assert_eq!((own.state, own.comments.len()), ("open", 0), "a person's issue is never touched");
}

// spec: BUILD-021
#[test]
fn build_021_a_finding_that_comes_back_reopens_its_issue() {
    let fake = Fake::start();
    fake.pass(&one_vuln());
    fake.pass(&audit(vec![], vec![]));
    let out = fake.pass(&one_vuln());
    assert!(out.contains("reopened #2"), "{out}");
    let issue = fake.report("RUSTSEC-2026-0001");
    assert_eq!(issue.state, "open");
    assert!(issue.comments[1].contains("reports RUSTSEC-2026-0001 again at abc1234"), "{:?}", issue.comments);
}

// spec: BUILD-021
#[test]
fn build_021_a_finding_closed_as_not_planned_stays_closed() {
    let fake = Fake::start();
    fake.pass(&one_vuln());
    fake.set_reason(2, "closed", "not_planned");
    let writes = fake.writes();
    let out = fake.pass(&one_vuln());
    assert!(out.contains("nothing new"), "{out}");
    assert_eq!(fake.writes(), writes);
    assert_eq!(fake.report("RUSTSEC-2026-0001").state, "closed");
}

// spec: BUILD-021
#[test]
fn build_021_markers_on_a_later_page_of_issues_are_found() {
    let fake = Fake::start();
    fake.pass(&one_vuln());
    {
        let mut s = fake.state.lock().unwrap();
        for n in 3..=150 {
            s.issues.push(Issue { number: n, title: format!("other {n}"), body: String::new(), labels: vec!["security".into()], state: "closed", reason: Some("completed".into()), comments: vec![] });
        }
        // the report becomes the newest, on the second page
        let report = s.issues.remove(1);
        s.issues.push(report);
    }
    let writes = fake.writes();
    let out = fake.pass(&one_vuln());
    assert!(out.contains("nothing new"), "{out}");
    assert_eq!(fake.writes(), writes);
}

// spec: BUILD-021
#[test]
fn build_021_a_missing_setting_or_bad_input_fails_before_any_request() {
    let fake = Fake::start();
    let ran = fake.script_with(&one_vuln(), |c| {
        c.env_remove("GH_TOKEN");
    });
    assert_error(&ran, "MIRROR_DEPLOY_KEY");
    let ran = fake.script_with(&json!({"no": "audit"}), |_| {});
    assert_error(&ran, "not the JSON of cargo audit --json");
    assert!(fake.state.lock().unwrap().log.is_empty(), "a request was made");
}

// spec: BUILD-021
#[test]
fn build_021_a_token_that_may_not_write_issues_names_the_secret() {
    let fake = Fake::start();
    let ran = fake.script_with(&one_vuln(), |c| {
        c.env("GH_TOKEN", "a-token-github-does-not-know");
    });
    assert_error(&ran, "401");
    assert!(ran.out.contains("MIRROR_DEPLOY_KEY") && !ran.out.contains("a-token-github"), "{}", ran.out);
}

// spec: BUILD-021
#[test]
fn build_021_a_github_that_refuses_or_does_not_answer_fails_loudly() {
    let fake = Fake::start();
    fake.mode(Mode::Status(503));
    assert_error(&fake.script_with(&one_vuln(), |_| {}), "503");
    fake.mode(Mode::HangUp);
    assert_error(&fake.script_with(&one_vuln(), |_| {}), "not reachable");
    fake.mode(Mode::Normal);
    assert_eq!(fake.reports().len(), 0);
}
