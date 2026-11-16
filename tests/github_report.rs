//! `scripts/report-github-ci.sh` (BUILD-020) against a pretend GitHub and a pretend Forgejo: one
//! small server answers both, under `/gh` and `/fj`. The Forgejo side keeps its issues in memory,
//! so a second run of the script sees what the first one wrote. The script needs `sh`, `curl`
//! and `jq`; a missing one fails the test with a message (no silent skip).
#![cfg(unix)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{json, Value};

const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/report-github-ci.sh");
const TOKEN: &str = "secret-token";
const PREFIX: &str = "GitHub CI red on main";

// ---- what GitHub knows

#[derive(Clone)]
struct Run {
    id: u64,
    status: &'static str,
    conclusion: Option<&'static str>,
    jobs: Vec<(&'static str, &'static str)>,
}

fn run_url(id: u64) -> String {
    format!("https://github.com/rabalyn/shale/actions/runs/{id}")
}

fn job_url(id: u64, name: &str) -> String {
    format!("{}/job/{name}", run_url(id))
}

/// The commit of run `id`: 40 characters, whose first 7 are the id with leading zeros.
fn sha(id: u64) -> String {
    format!("{id:07}abcdef0123456789abcdef0123456789a")
}

fn short(id: u64) -> String {
    sha(id)[..7].to_string()
}

fn red(id: u64) -> Run {
    let jobs = vec![("macos", "success"), ("windows", "failure"), ("linux", "skipped"), ("audit", "skipped")];
    Run { id, status: "completed", conclusion: Some("failure"), jobs }
}

fn green(id: u64) -> Run {
    let jobs = vec![("macos", "success"), ("windows", "success"), ("linux", "skipped"), ("audit", "skipped")];
    Run { id, status: "completed", conclusion: Some("success"), jobs }
}

fn running(id: u64) -> Run {
    Run { id, status: "in_progress", conclusion: None, jobs: vec![] }
}

fn cancelled(id: u64) -> Run {
    Run { id, status: "completed", conclusion: Some("cancelled"), jobs: vec![] }
}

fn run_json(r: &Run) -> Value {
    json!({
        "id": r.id, "head_sha": sha(r.id), "status": r.status, "conclusion": r.conclusion,
        "html_url": run_url(r.id), "run_attempt": 1,
        "head_commit": {"message": format!("Change {} at 100%\n\nSecond line of the message", r.id)},
    })
}

fn jobs_json(r: &Run) -> Value {
    let jobs: Vec<Value> = r.jobs.iter().map(|(name, c)| json!({"name": name, "status": "completed", "conclusion": c, "html_url": job_url(r.id, name)})).collect();
    json!({"total_count": jobs.len(), "jobs": jobs})
}

enum Github {
    Runs(Vec<Run>),
    Status(u16),
    HangUp,
}

// ---- what Forgejo knows

#[derive(Clone)]
struct Issue {
    number: u64,
    title: String,
    body: String,
    open: bool,
    comments: Vec<String>,
}

fn issue_json(i: &Issue) -> Value {
    json!({"number": i.number, "title": i.title, "state": if i.open { "open" } else { "closed" }, "body": i.body})
}

fn param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
}

type Reply = Option<(u16, String)>;

fn reply(code: u16, v: Value) -> Reply {
    Some((code, v.to_string()))
}

struct State {
    github: Github,
    issues: Vec<Issue>,
    refuse_writes: bool,
    log: Vec<String>,
}

impl State {
    fn handle(&mut self, method: &str, target: &str, auth: Option<&str>, body: &str) -> Reply {
        self.log.push(format!("{method} {target}"));
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        if let Some(p) = path.strip_prefix("/gh/") {
            return self.github(p, query);
        }
        let Some(p) = path.strip_prefix("/fj/repos/raba/shale/") else {
            return reply(404, json!({}));
        };
        if auth != Some(format!("token {TOKEN}").as_str()) {
            return reply(401, json!({"message": "token is required"}));
        }
        if method != "GET" && self.refuse_writes {
            return reply(403, json!({"message": "token does not have write:issue scope"}));
        }
        self.forgejo(method, p, query, body)
    }

    fn github(&self, path: &str, query: &str) -> Reply {
        let runs = match &self.github {
            Github::HangUp => return None,
            Github::Status(code) => return reply(*code, json!({"message": "no"})),
            Github::Runs(runs) => runs,
        };
        if path == "repos/rabalyn/shale/actions/workflows/build.yml/runs" && param(query, "branch") == Some("main") && param(query, "event") == Some("push") {
            let mut newest_first = runs.clone();
            newest_first.sort_by_key(|r| std::cmp::Reverse(r.id));
            let list: Vec<Value> = newest_first.iter().map(run_json).collect();
            return reply(200, json!({"total_count": list.len(), "workflow_runs": list}));
        }
        let id = path.strip_prefix("repos/rabalyn/shale/actions/runs/").and_then(|p| p.strip_suffix("/jobs"));
        match runs.iter().find(|r| Some(r.id.to_string().as_str()) == id) {
            Some(r) => reply(200, jobs_json(r)),
            None => reply(404, json!({"message": "Not Found"})),
        }
    }

    fn forgejo(&mut self, method: &str, path: &str, query: &str, body: &str) -> Reply {
        let sent: Value = serde_json::from_str(body).unwrap_or_default();
        let text = |key: &str| sent[key].as_str().unwrap_or_default().to_string();
        let parts: Vec<&str> = path.split('/').collect();
        if let ("GET", ["issues"]) = (method, parts.as_slice()) {
            let wanted = param(query, "state").unwrap_or("open");
            let all: Vec<Value> = self.issues.iter().filter(|i| wanted == "all" || (wanted == "open") == i.open).map(issue_json).collect();
            return reply(200, Value::Array(all));
        }
        if let ("POST", ["issues"]) = (method, parts.as_slice()) {
            let number = self.issues.len() as u64 + 1;
            let issue = Issue { number, title: text("title"), body: text("body"), open: true, comments: vec![] };
            let created = issue_json(&issue);
            self.issues.push(issue);
            return reply(201, created);
        }
        let number = parts.get(1).and_then(|n| n.parse::<u64>().ok());
        let Some(issue) = self.issues.iter_mut().find(|i| Some(i.number) == number) else {
            return reply(404, json!({"message": "issue does not exist"}));
        };
        match (method, parts.as_slice()) {
            ("GET", ["issues", _]) => reply(200, issue_json(issue)),
            ("PATCH", ["issues", _]) => {
                issue.open = text("state") != "closed";
                reply(201, issue_json(issue))
            }
            ("GET", ["issues", _, "comments"]) => {
                let limit: usize = param(query, "limit").and_then(|l| l.parse().ok()).unwrap_or(30);
                let page: usize = param(query, "page").and_then(|p| p.parse().ok()).unwrap_or(1);
                let slice: Vec<Value> = issue.comments.iter().skip((page - 1) * limit).take(limit).map(|b| json!({"body": b})).collect();
                reply(200, Value::Array(slice))
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
    assert!(Command::new(tool).arg("--version").output().is_ok(), "`{tool}` is not installed: scripts/report-github-ci.sh and this test (BUILD-020) need it");
}

impl Fake {
    /// A server whose Forgejo side already has two issues that are not the report: an old one, and
    /// one whose title only contains the report's title and which carries a marker with a very
    /// high run id (so that a title that is not checked at its start would make everything look done).
    fn start() -> Fake {
        for tool in ["sh", "curl", "jq"] {
            need(tool);
        }
        let issue = |number, title: &str, body: &str| Issue { number, title: title.into(), body: body.into(), open: true, comments: vec![] };
        let state = Arc::new(Mutex::new(State {
            github: Github::Runs(vec![]),
            issues: vec![issue(1, "The start is slow", "no marker here"), issue(2, &format!("Re: {PREFIX}: windows failed at 0000000"), "<!-- shale-github-run:999999:failure -->")],
            refuse_writes: false,
            log: vec![],
        }));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::spawn({
            let (state, stop) = (state.clone(), stop.clone());
            move || serve(listener, state, stop)
        });
        Fake { addr, state, stop, thread: Some(thread) }
    }

    fn runs(&self, runs: Vec<Run>) {
        self.state.lock().unwrap().github = Github::Runs(runs);
    }

    fn github(&self, github: Github) {
        self.state.lock().unwrap().github = github;
    }

    fn refuse_writes(&self) {
        self.state.lock().unwrap().refuse_writes = true;
    }

    fn seed(&self, issue: Issue) {
        self.state.lock().unwrap().issues.push(issue);
    }

    /// The issues made by the script (the title starts with the report's title).
    fn reports(&self) -> Vec<Issue> {
        self.state.lock().unwrap().issues.iter().filter(|i| i.title.starts_with(PREFIX)).cloned().collect()
    }

    fn issue_count(&self) -> usize {
        self.state.lock().unwrap().issues.len()
    }

    fn log(&self) -> Vec<String> {
        self.state.lock().unwrap().log.clone()
    }

    /// Requests that change something (not GET).
    fn writes(&self) -> usize {
        self.log().iter().filter(|l| !l.starts_with("GET ")).count()
    }

    fn script(&self) -> Ran {
        self.script_with(|_| {})
    }

    fn script_with(&self, tweak: impl FnOnce(&mut Command)) -> Ran {
        let mut c = Command::new("sh");
        c.arg(SCRIPT);
        for var in ["GITHUB_API", "GITHUB_REPO", "GITHUB_WORKFLOW", "FORGEJO_API", "FORGEJO_REPO", "FORGEJO_TOKEN"] {
            c.env_remove(var);
        }
        for var in ["http_proxy", "https_proxy", "all_proxy", "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
            c.env_remove(var);
        }
        c.env("NO_PROXY", "*").env("no_proxy", "*");
        c.env("GITHUB_API", format!("http://{}/gh", self.addr));
        c.env("FORGEJO_API", format!("http://{}/fj", self.addr));
        c.env("FORGEJO_REPO", "raba/shale").env("FORGEJO_TOKEN", TOKEN);
        tweak(&mut c);
        let output = c.output().expect("sh starts");
        let out = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        Ran { ok: output.status.success(), out }
    }

    /// The script runs and succeeds.
    fn pass(&self) -> String {
        let ran = self.script();
        assert!(ran.ok, "the script failed:\n{}", ran.out);
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
    }
}

/// The script fails with an `::error::` line that contains `needle`.
fn assert_error(ran: &Ran, needle: &str) {
    assert!(!ran.ok, "the script should fail:\n{}", ran.out);
    let line = ran.out.lines().find(|l| l.starts_with("::error::"));
    assert!(line.is_some_and(|l| l.contains(needle)), "no ::error:: line with {needle:?}:\n{}", ran.out);
}

// ---- the tests

// spec: BUILD-020
#[test]
fn build_020_a_red_run_opens_one_issue_with_the_failed_job() {
    let fake = Fake::start();
    fake.runs(vec![red(10)]);
    let out = fake.pass();
    assert!(out.contains("opened issue #3"), "{out}");
    let reports = fake.reports();
    assert_eq!(reports.len(), 1);
    let issue = &reports[0];
    assert_eq!(issue.title, format!("{PREFIX}: windows failed at {}", short(10)));
    assert!(issue.open);
    for wanted in [run_url(10), sha(10), "Change 10 at 100%\n".to_string(), format!("- windows: failure ({})", job_url(10, "windows")), "<!-- shale-github-run:10:failure -->".to_string()] {
        assert!(issue.body.contains(&wanted), "{wanted:?} is missing in:\n{}", issue.body);
    }
    for unwanted in ["macos", "linux", "audit", "Second line"] {
        assert!(!issue.body.contains(unwanted), "{unwanted:?} is in:\n{}", issue.body);
    }
}

// spec: BUILD-020
#[test]
fn build_020_a_run_that_was_reported_is_not_reported_again() {
    let fake = Fake::start();
    fake.runs(vec![red(10)]);
    fake.pass();
    let writes = fake.writes();
    let out = fake.pass();
    assert!(out.contains("nothing new"), "{out}");
    assert_eq!(fake.writes(), writes, "the second pass wrote something");
    assert_eq!(fake.reports().len(), 1);
    assert!(fake.reports()[0].comments.is_empty());
}

// spec: BUILD-020
#[test]
fn build_020_a_newer_red_run_is_a_comment_on_the_open_issue() {
    let fake = Fake::start();
    fake.runs(vec![red(10)]);
    fake.pass();
    fake.runs(vec![red(10), red(11)]);
    let out = fake.pass();
    assert!(out.contains("commented on #3"), "{out}");
    let reports = fake.reports();
    assert_eq!(reports.len(), 1, "a new issue was opened");
    assert_eq!(reports[0].comments.len(), 1);
    assert!(reports[0].comments[0].contains("<!-- shale-github-run:11:failure -->"));
    assert!(reports[0].comments[0].contains(&job_url(11, "windows")));
    let writes = fake.writes();
    fake.pass();
    assert_eq!(fake.writes(), writes, "the third pass wrote something");
}

// spec: BUILD-020
#[test]
fn build_020_a_green_run_closes_the_issue_and_the_next_red_run_opens_a_new_one() {
    let fake = Fake::start();
    fake.runs(vec![red(10)]);
    fake.pass();
    fake.runs(vec![red(10), green(12)]);
    let out = fake.pass();
    assert!(out.contains("closed #3"), "{out}");
    let reports = fake.reports();
    assert!(!reports[0].open);
    assert_eq!(reports[0].comments.len(), 1);
    assert!(reports[0].comments[0].contains("green again"));
    assert!(reports[0].comments[0].contains("Change 12 at 100%\n"), "{}", reports[0].comments[0]);
    assert!(reports[0].comments[0].contains("<!-- shale-github-run:12:success -->"));

    // a further green run finds no open issue: nothing is written
    fake.runs(vec![red(10), green(12), green(13)]);
    let writes = fake.writes();
    fake.pass();
    assert_eq!(fake.writes(), writes, "a green run wrote to the closed issue");

    fake.runs(vec![red(10), green(12), green(13), red(14)]);
    fake.pass();
    let reports = fake.reports();
    assert_eq!(reports.len(), 2, "the closed issue was reused or the old red run was reported again");
    assert!(!reports[0].open && reports[0].comments.len() == 1);
    assert!(reports[1].open && reports[1].number == 4);
    assert_eq!(reports[1].title, format!("{PREFIX}: windows failed at {}", short(14)));
}

// spec: BUILD-020
#[test]
fn build_020_green_runs_without_an_issue_write_nothing() {
    let fake = Fake::start();
    fake.runs(vec![green(1), green(2)]);
    let out = fake.pass();
    assert!(out.contains("nothing new"), "{out}");
    assert_eq!(fake.writes(), 0);
    assert_eq!(fake.issue_count(), 2);
}

// spec: BUILD-020
#[test]
fn build_020_a_run_that_is_still_running_is_waited_for() {
    // the newest run is still running: the red one before it is reported, the running one is not
    let fake = Fake::start();
    fake.runs(vec![red(10)]);
    fake.pass();
    fake.runs(vec![red(10), running(11)]);
    let writes = fake.writes();
    let out = fake.pass();
    assert!(out.contains("run 11 is still running"), "{out}");
    assert_eq!(fake.writes(), writes);

    // a running run followed by a newer red one: the red one waits, so the order is kept
    let fake = Fake::start();
    fake.runs(vec![running(10), red(11)]);
    let out = fake.pass();
    assert!(out.contains("run 10 is still running"), "{out}");
    assert_eq!(fake.writes(), 0, "the later red run was reported before the running one");
    fake.runs(vec![red(10), red(11)]);
    fake.pass();
    let reports = fake.reports();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].title.ends_with(&short(10)), "{}", reports[0].title);
    assert_eq!(reports[0].comments.len(), 1);
    assert!(reports[0].comments[0].contains("shale-github-run:11:failure"));
}

// spec: BUILD-020
#[test]
fn build_020_no_run_at_all_is_nothing_to_report() {
    let fake = Fake::start();
    let out = fake.pass();
    assert!(out.contains("no run"), "{out}");
    assert_eq!(fake.writes(), 0);
}

// spec: BUILD-020
#[test]
fn build_020_the_first_use_reports_only_the_current_red_streak() {
    // the history red, red, green: nothing to report
    let fake = Fake::start();
    fake.runs(vec![red(1), red(2), green(3)]);
    fake.pass();
    assert_eq!(fake.writes(), 0);

    // green, red: the red one
    let fake = Fake::start();
    fake.runs(vec![green(1), red(2)]);
    fake.pass();
    let reports = fake.reports();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].title.ends_with(&short(2)));

    // red, red after a green one: both belong to one issue, the older red one before the green is not in it
    let fake = Fake::start();
    fake.runs(vec![red(1), green(2), red(3), red(4)]);
    fake.pass();
    let reports = fake.reports();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].title.ends_with(&short(3)));
    assert_eq!(reports[0].comments.len(), 1);
}

// spec: BUILD-020
#[test]
fn build_020_a_cancelled_run_is_ignored() {
    let fake = Fake::start();
    fake.runs(vec![cancelled(3)]);
    fake.pass();
    assert_eq!(fake.writes(), 0);
    fake.runs(vec![red(4), cancelled(5)]);
    fake.pass();
    let reports = fake.reports();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].comments.is_empty(), "the cancelled run was reported");
}

// spec: BUILD-020
#[test]
fn build_020_a_rerun_that_turned_a_reported_run_green_closes_the_issue() {
    let fake = Fake::start();
    fake.runs(vec![red(10)]);
    fake.pass();
    fake.runs(vec![green(10)]);
    let out = fake.pass();
    assert!(out.contains("closed #3: run 10 is green"), "{out}");
    let reports = fake.reports();
    assert!(!reports[0].open);
    assert!(reports[0].comments[0].contains("<!-- shale-github-run:10:success -->"));
    let writes = fake.writes();
    fake.pass();
    assert_eq!(fake.writes(), writes, "the green run was handled twice");

    // and the other way round: a re-run that turned the green run red is reported (a new issue, the old one is closed)
    fake.runs(vec![red(10)]);
    fake.pass();
    let reports = fake.reports();
    assert_eq!(reports.len(), 2, "{:?}", reports.iter().map(|r| &r.title).collect::<Vec<_>>());
    assert!(!reports[0].open && reports[1].open && reports[1].title.ends_with(&short(10)));
}

// spec: BUILD-020
#[test]
fn build_020_a_red_run_without_a_failed_job_is_still_reported() {
    let fake = Fake::start();
    fake.runs(vec![Run { jobs: vec![], ..red(10) }]);
    fake.pass();
    let reports = fake.reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].title, format!("{PREFIX}: run failed at {}", short(10)));
    assert!(reports[0].body.contains("No failed job is listed"), "{}", reports[0].body);
    assert!(reports[0].body.contains(&run_url(10)));
}

// spec: BUILD-020
#[test]
fn build_020_a_github_that_does_not_answer_fails_the_job_and_writes_nothing() {
    let fake = Fake::start();
    fake.github(Github::Status(503));
    assert_error(&fake.script(), "503");

    fake.github(Github::Status(403));
    assert_error(&fake.script(), "rate limit");

    fake.github(Github::HangUp);
    assert_error(&fake.script(), "not reachable");

    assert_eq!(fake.writes(), 0);
    assert!(fake.log().iter().all(|l| l.contains("/gh/")), "Forgejo was asked: {:?}", fake.log());
}

// spec: BUILD-020
#[test]
fn build_020_a_missing_setting_fails_before_any_request() {
    let fake = Fake::start();
    fake.runs(vec![red(10)]);
    assert_error(
        &fake.script_with(|c| {
            c.env("FORGEJO_TOKEN", "");
        }),
        "FORGEJO_TOKEN",
    );
    assert_error(
        &fake.script_with(|c| {
            c.env_remove("FORGEJO_TOKEN");
        }),
        "FORGEJO_TOKEN",
    );
    assert_error(
        &fake.script_with(|c| {
            c.env_remove("FORGEJO_API");
        }),
        "FORGEJO_API",
    );
    assert_error(
        &fake.script_with(|c| {
            c.env_remove("FORGEJO_REPO");
        }),
        "FORGEJO_REPO",
    );
    assert!(fake.log().is_empty(), "a request was made: {:?}", fake.log());
}

// spec: BUILD-020
#[test]
fn build_020_a_token_that_may_not_write_issues_names_the_secret() {
    let fake = Fake::start();
    fake.runs(vec![red(10)]);
    fake.refuse_writes();
    let ran = fake.script();
    assert_error(&ran, "ISSUE_REPORT_TOKEN");
    assert!(ran.out.contains("403"), "{}", ran.out);
    assert_eq!(fake.reports().len(), 0);

    let ran = fake.script_with(|c| {
        c.env("FORGEJO_TOKEN", "a-token-forgejo-does-not-know");
    });
    assert_error(&ran, "ISSUE_REPORT_TOKEN");
    assert!(ran.out.contains("401") && !ran.out.contains("a-token-forgejo"), "{}", ran.out);
}

// spec: BUILD-020
#[test]
fn build_020_markers_on_a_later_page_of_comments_are_found() {
    let fake = Fake::start();
    let mut comments: Vec<String> = (1..60).map(|n| format!("a comment of someone, number {n}")).collect();
    comments.push("Run 5 is red too.\n\n<!-- shale-github-run:5:failure -->".into());
    fake.seed(Issue { number: 3, title: format!("{PREFIX}: windows failed at {}", short(4)), body: "<!-- shale-github-run:4:failure -->".into(), open: true, comments });
    fake.runs(vec![red(4), red(5)]);
    let out = fake.pass();
    assert!(out.contains("nothing new"), "{out}");
    assert_eq!(fake.writes(), 0, "run 5 was reported again: its marker is on the second page");
    assert!(fake.log().iter().any(|l| l.contains("comments?page=2")), "{:?}", fake.log());
}
