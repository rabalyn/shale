//! Downloading a file with `curl` (the way the CalDAV client does its HTTP): no `.curlrc`, a size
//! limit, a timeout, redirects only to https, and a flag that stops it. Shared by the TeX Live
//! install (TEX-002) and the update check (UPD-003); both run it in a background job.

use crate::pdf::tex::proc::{self, Ending};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// One download.
pub struct Request<'a> {
    pub url: &'a str,
    /// The most bytes curl accepts.
    pub max_bytes: u64,
    /// How long the whole download may take.
    pub timeout: Duration,
    /// `http://` is accepted for `url` (a pretend server in a test); otherwise only `https://`.
    /// A redirect may only lead to https either way.
    pub allow_http: bool,
    /// Request headers (`Accept: …`), never secrets: they are command line arguments.
    pub headers: &'a [&'a str],
}

/// The curl protocol list for `url`: https, and http too when the request allows it and the address is http.
fn protos(r: &Request) -> &'static str {
    if r.allow_http && r.url.starts_with("http://") {
        "=http,https"
    } else {
        "=https"
    }
}

/// The curl command of a request (without the output file).
fn command(r: &Request) -> Command {
    let mut c = crate::program::command("curl");
    c.args(["-q", "-sS", "-f", "-L", "--max-redirs", "5", "--proto", protos(r), "--proto-redir", "=https", "--connect-timeout", "20", "--max-filesize"]).arg(r.max_bytes.to_string());
    for h in r.headers {
        c.arg("-H").arg(h);
    }
    c
}

/// Download `r.url` to the file `to`. The error says in words what went wrong ("cancelled" when
/// `cancel` was set).
pub fn to_file(cancel: &AtomicBool, r: &Request, to: &Path) -> Result<(), String> {
    let mut c = command(r);
    c.arg("-o").arg(to).arg(r.url);
    let run = proc::run(c, r.timeout, Some(cancel), |_| {}).map_err(|e| format!("could not run curl ({e}) — is curl installed?"))?;
    match run.ending {
        Ending::Cancelled => Err("cancelled".into()),
        Ending::TimedOut => Err("the download took too long and was stopped".into()),
        Ending::Exited(Some(0)) => Ok(()),
        Ending::Exited(_) => Err(format!("the download of {} failed: {}", r.url, run.tail_text())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req<'a>(url: &'a str, allow_http: bool) -> Request<'a> {
        Request { url, max_bytes: 10, timeout: Duration::from_secs(5), allow_http, headers: &[] }
    }

    // spec: UPD-003, UPD-021
    #[test]
    fn http_is_accepted_only_when_the_request_allows_it_and_redirects_only_lead_to_https() {
        assert_eq!(protos(&req("https://example.org/a", false)), "=https");
        assert_eq!(protos(&req("https://example.org/a", true)), "=https");
        assert_eq!(protos(&req("http://127.0.0.1:1/a", false)), "=https");
        assert_eq!(protos(&req("http://127.0.0.1:1/a", true)), "=http,https");
        let args: Vec<String> = command(&req("https://example.org/a", false)).get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(args.windows(2).any(|w| w == ["--proto-redir", "=https"]) && args.contains(&"-q".to_string()), "{args:?}");
    }

    // spec: UPD-003, UPD-021
    #[test]
    fn a_refused_http_address_fails_in_words_and_writes_nothing() {
        let to = std::env::temp_dir().join(format!("shale-download-{}", std::process::id()));
        let _ = std::fs::remove_file(&to);
        let e = to_file(&AtomicBool::new(false), &req("http://127.0.0.1:9/a", false), &to).unwrap_err();
        assert!(e.contains("failed") || e.contains("curl"), "{e}");
        assert!(!to.exists());
    }

    // spec: UPD-021
    #[test]
    fn a_file_bigger_than_the_size_limit_is_refused_and_a_silent_server_ends_at_the_timeout() {
        use std::io::Write;
        // a server that announces 100 bytes and sends them
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/a", listener.local_addr().unwrap().port());
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{}", "x".repeat(100)).as_bytes());
        });
        let to = std::env::temp_dir().join(format!("shale-download-s-{}", std::process::id()));
        let _ = std::fs::remove_file(&to);
        let e = to_file(&AtomicBool::new(false), &req(&url, true), &to).unwrap_err();
        assert!(e.contains("failed"), "{e}");
        server.join().unwrap();
        // a server that accepts and then says nothing: the timeout stops the download
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/a", listener.local_addr().unwrap().port());
        let held = std::thread::spawn(move || listener.accept().map(|(s, _)| s));
        let e = to_file(&AtomicBool::new(false), &Request { timeout: Duration::from_millis(500), ..req(&url, true) }, &to).unwrap_err();
        assert!(e.contains("took too long"), "{e}");
        drop(held);
        let _ = std::fs::remove_file(&to);
    }

    // spec: UPD-003, UPD-021
    #[test]
    fn a_cancel_flag_stops_a_download_that_gets_no_answer() {
        use std::sync::Arc;
        // a server that accepts and then says nothing
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/a", listener.local_addr().unwrap().port());
        let (accepted, was_accepted) = std::sync::mpsc::channel();
        let held = std::thread::spawn(move || {
            let s = listener.accept().map(|(s, _)| s);
            let _ = accepted.send(());
            s
        });
        let to = std::env::temp_dir().join(format!("shale-download-c-{}", std::process::id()));
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        std::thread::spawn(move || {
            // the server has the connection and says nothing: now the user cancels
            if was_accepted.recv().is_ok() {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        });
        let e = to_file(&cancel, &Request { timeout: Duration::from_secs(60), ..req(&url, true) }, &to).unwrap_err();
        assert_eq!(e, "cancelled");
        drop(held);
    }
}
