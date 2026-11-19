//! The download of the newest tuda-ci from CTAN (TEX-006). `mirrors.ctan.org` is a redirector: it
//! sends each request to some mirror, and a mirror may be down or out of date. So the redirect is
//! followed once to a concrete mirror, the file is taken from it, and a failure that another mirror
//! can fix (it cannot be connected to, answers 403, 404 or 5xx, sends a broken zip) is retried on
//! a fresh mirror, [`MIRROR_TRIES`] tries in all, as the TeX Live install does (TEX-002). A mirror
//! the redirector names again is not asked again, but the try counts. An address that does not
//! redirect is its own mirror and is tried once.

use super::archive;
use super::install::{download, Handle, Stage, MIRROR_TRIES};
use super::proc::{self, Ending};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Where `url` leads: the address it redirects to, or `None` for an address that answers itself.
/// An error when the address cannot be reached at all (no mirror can help then).
fn redirect_target(handle: &Handle, url: &str) -> Result<Option<String>, String> {
    let protos = if url.starts_with("http://") { "=http,https" } else { "=https" };
    let mut c = Command::new("curl");
    c.args(["-q", "-sS", "--proto", protos, "--proto-redir", protos, "--connect-timeout", "20", "--max-time", "60", "-o"]).arg(null_device()).args(["-w", "%{http_code} %{redirect_url}\n"]).arg(url);
    let run = proc::run(c, Duration::from_secs(120), Some(&handle.cancel), |_| {}).map_err(|e| format!("could not run curl ({e}) — is curl installed?"))?;
    match run.ending {
        Ending::Cancelled => Err("cancelled".into()),
        Ending::TimedOut => Err(format!("{url} did not answer")),
        Ending::Exited(Some(0)) => {
            let line = run.tail.iter().rev().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("");
            let (code, to) = line.split_once(' ').unwrap_or((line, ""));
            let to = to.trim();
            let https_only = !url.starts_with("http://");
            let allowed = to.starts_with("https://") || (!https_only && to.starts_with("http://"));
            if code.starts_with('3') && !to.is_empty() {
                if allowed {
                    Ok(Some(to.to_string()))
                } else {
                    Err(format!("{url} sent the download to {to}, which is not an https address"))
                }
            } else {
                Ok(None)
            }
        }
        Ending::Exited(_) => Err(format!("{url} could not be reached: {}", run.tail_text())),
    }
}

fn null_device() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}

/// The curl exit code and the HTTP status in a download error, if it has them.
fn curl_code(err: &str) -> Option<u32> {
    let rest = &err[err.find("curl: (")? + "curl: (".len()..];
    rest[..rest.find(')')?].parse().ok()
}

fn http_status(err: &str) -> Option<u32> {
    let rest = &err[err.find("returned error: ")? + "returned error: ".len()..];
    rest.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

/// Whether another mirror can fix a failed download: the mirror cannot be reached (curl exit codes
/// for resolving, connecting, timeouts, TLS and broken transfers) or it answered 403, 404 or 5xx.
pub(super) fn another_mirror_can_help(err: &str) -> bool {
    if err.starts_with("the download took too long") {
        return true;
    }
    match curl_code(err) {
        Some(22) => http_status(err).is_some_and(|s| s == 403 || s == 404 || s >= 500),
        Some(c) => matches!(c, 6 | 7 | 18 | 28 | 35 | 51 | 52 | 55 | 56 | 60 | 92),
        None => false,
    }
}

/// What went wrong with one mirror, in plain words.
fn describe(err: &str) -> String {
    if err.starts_with("the download took too long") {
        return "it took too long".into();
    }
    match curl_code(err) {
        Some(6) => "its name could not be resolved".into(),
        Some(7) => "it could not be connected to".into(),
        Some(28) => "it did not answer in time".into(),
        Some(35 | 51 | 60) => "no secure connection could be made to it".into(),
        Some(18 | 52 | 55 | 56 | 92) => "the transfer broke off".into(),
        Some(22) => http_status(err).map_or_else(|| "it refused the download".into(), |s| format!("it answered with the error {s}")),
        _ => err.split_once("failed: ").map_or(err, |(_, why)| why).trim().to_string(),
    }
}

/// Download `url` to `archive_file` and unpack it into the empty folder `src` (the stages Fetch and
/// Read). Gives the mirror the file came from. See the module for the tries.
pub(super) fn fetch(handle: &Handle, url: &str, archive_file: &Path, src: &Path) -> Result<String, String> {
    let mut failed: Vec<(String, String)> = vec![];
    for attempt in 1..=MIRROR_TRIES {
        handle.stage(Stage::Fetch);
        let redirected = redirect_target(handle, url).map_err(|e| with_failures(e, &failed))?;
        let mirror = redirected.clone().unwrap_or_else(|| url.to_string());
        if failed.iter().any(|(m, _)| *m == mirror) {
            failed.push((mirror, "the redirector named it again, so it was not asked again".into()));
            continue;
        }
        let got = download(handle, &mirror, archive_file).and_then(|()| {
            let bytes = crate::fsutil::read(archive_file).map_err(|e| e.to_string())?;
            handle.stage(Stage::Read);
            archive::extract(&bytes, src).map_err(|e| format!("the archive is broken: {e}"))
        });
        match got {
            Ok(()) => return Ok(mirror),
            Err(e) if redirected.is_none() || !(another_mirror_can_help(&e) || e.starts_with("the archive is broken")) => return Err(if redirected.is_none() { e } else { with_failures(e, &failed) }),
            Err(e) => {
                let why = if e.starts_with("the archive is broken") { "it sent a broken archive".to_string() } else { describe(&e) };
                failed.push((mirror, why));
                if attempt == MIRROR_TRIES {
                    break;
                }
            }
        }
    }
    Err(with_failures(String::new(), &failed))
}

/// The error with the mirrors tried before it, one per line.
fn with_failures(last: String, failed: &[(String, String)]) -> String {
    if failed.is_empty() {
        return last;
    }
    let list: Vec<String> = failed.iter().map(|(m, why)| format!("- {m}: {why}")).collect();
    let head = format!("the download from CTAN failed on {} mirror{}:\n{}", failed.len(), if failed.len() == 1 { "" } else { "s" }, list.join("\n"));
    if last.is_empty() {
        head
    } else {
        format!("{head}\nand then: {last}")
    }
}
