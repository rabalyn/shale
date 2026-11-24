//! The CTAN download of the template-set import (TEX-006): the redirector is resolved to a mirror,
//! a dead mirror is left for another, three tries in all. The mirrors and the redirector are
//! pretend web servers on 127.0.0.1; a dead mirror is a port nobody listens on.

use super::archive_tests::{z, zip};
use super::ctan::fetch;
use super::install::{Handle, MIRROR_TRIES};
use crate::calendar::dav::tests::{closed_port, ClosedPort};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

enum Reply {
    Body(Vec<u8>),
    Redirect(String),
    Status(u16),
}

/// A web server that answers every request with `answer(path)`; gives its address and the log of paths asked.
fn serve(answer: impl Fn() -> Reply + Send + 'static) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let asked = Arc::new(Mutex::new(vec![]));
    let log = asked.clone();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let first = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string();
            log.lock().unwrap().push(first.split_whitespace().nth(1).unwrap_or("").to_string());
            let (head, body) = match answer() {
                Reply::Body(b) => (format!("HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n", b.len()), b),
                Reply::Redirect(to) => (format!("HTTP/1.0 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\n\r\n"), vec![]),
                Reply::Status(code) => (format!("HTTP/1.0 {code} Nope\r\nContent-Length: 0\r\n\r\n"), vec![]),
            };
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    (url, asked)
}

fn good_zip() -> Vec<u8> {
    zip(&[z("tuda-ci/tudapub.cls", b"% class")])
}

/// A mirror with the file.
fn mirror() -> (String, Arc<Mutex<Vec<String>>>) {
    serve(|| Reply::Body(good_zip()))
}

/// An address nobody listens on: connecting fails at once. The port is held closed while the
/// second value lives (a released port could be handed to another test's server, #659).
fn dead() -> (String, ClosedPort) {
    let held = closed_port();
    (format!("http://127.0.0.1:{}", held.port), held)
}

/// A pretend `mirrors.ctan.org`: each request is sent to the next of `mirrors`, in turn.
fn redirector(mirrors: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
    let next = AtomicU32::new(0);
    serve(move || Reply::Redirect(format!("{}/tuda-ci.zip", mirrors[next.fetch_add(1, Ordering::SeqCst) as usize % mirrors.len()])))
}

fn dirs(tag: &str) -> (PathBuf, PathBuf) {
    let d = std::env::temp_dir().join(format!("shale-ctan-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let src = d.join("src");
    std::fs::create_dir_all(&src).unwrap();
    (d.join("archive"), src)
}

fn run(url: &str, tag: &str) -> (Result<String, String>, PathBuf) {
    let (archive, src) = dirs(tag);
    (fetch(&Handle::default(), url, &archive, &src), src)
}

fn asked(log: &Arc<Mutex<Vec<String>>>) -> usize {
    log.lock().unwrap().len()
}

// spec: TEX-006
#[test]
fn a_dead_first_mirror_is_left_for_the_second_which_the_import_uses() {
    let (good, got) = mirror();
    let (dead, _held) = dead();
    let (red, _) = redirector(vec![dead.clone(), good.clone()]);
    let (r, src) = run(&format!("{red}/macros/tuda-ci.zip"), "second");
    assert_eq!(r, Ok(format!("{good}/tuda-ci.zip")), "the mirror the file came from");
    assert!(src.join("tuda-ci/tudapub.cls").exists());
    assert_eq!(asked(&got), 1);
}

// spec: TEX-006
#[test]
fn three_dead_mirrors_give_an_error_that_names_all_three() {
    let ((a, _ha), (b, _hb), (c, _hc)) = (dead(), dead(), dead());
    let (red, asked_red) = redirector(vec![a.clone(), b.clone(), c.clone()]);
    let (r, _) = run(&format!("{red}/x.zip"), "three-dead");
    let e = r.unwrap_err();
    for m in [&a, &b, &c] {
        assert!(e.contains(m.as_str()) && e.contains("could not be connected to"), "{e}");
    }
    assert!(e.contains("3 mirrors"), "{e}");
    assert_eq!(asked(&asked_red), MIRROR_TRIES, "one question per try, no more tries");
}

// spec: TEX-006
#[test]
fn a_mirror_the_redirector_names_again_is_skipped_but_the_try_counts() {
    let (bad, asked_bad) = serve(|| Reply::Status(404));
    let (red, asked_red) = redirector(vec![bad.clone(), bad.clone(), bad.clone()]);
    let (r, _) = run(&format!("{red}/x.zip"), "again");
    let e = r.unwrap_err();
    assert_eq!(asked(&asked_red), MIRROR_TRIES, "the repeats counted as tries");
    assert_eq!(asked(&asked_bad), 1, "the repeated mirror was not asked again");
    assert!(e.contains("answered with the error 404") && e.contains("named it again"), "{e}");
}

// spec: TEX-006
#[test]
fn a_redirector_that_cannot_be_reached_fails_at_once() {
    let (dead, _held) = dead();
    let (r, _) = run(&format!("{dead}/x.zip"), "no-redirector");
    let e = r.unwrap_err();
    assert!(e.contains("could not be reached") && !e.contains("mirror"), "{e}");
}

// spec: TEX-006
#[test]
fn an_address_that_does_not_redirect_is_its_own_mirror_and_is_tried_once() {
    let (bad, asked_bad) = serve(|| Reply::Status(503));
    let (r, _) = run(&format!("{bad}/x.zip"), "direct");
    let e = r.unwrap_err();
    assert!(e.contains("503"), "{e}");
    assert_eq!(asked(&asked_bad), 2, "one question whether it redirects, one download, no second try");
    let (good, _) = mirror();
    let (r, _) = run(&format!("{good}/x.zip"), "direct-ok");
    assert_eq!(r, Ok(format!("{good}/x.zip")));
}

// spec: TEX-006
#[test]
fn a_broken_archive_is_a_failure_of_that_mirror_and_the_next_one_is_tried() {
    let (broken, _) = serve(|| Reply::Body(b"PK this is cut off".to_vec()));
    let (good, _) = mirror();
    let (red, _) = redirector(vec![broken, good.clone()]);
    let (r, src) = run(&format!("{red}/x.zip"), "broken");
    assert_eq!(r, Ok(format!("{good}/tuda-ci.zip")));
    assert!(src.join("tuda-ci/tudapub.cls").exists());
}

// spec: TEX-006
#[test]
fn which_failures_another_mirror_can_fix() {
    use super::ctan::another_mirror_can_help as can;
    assert!(can("the download of u failed: curl: (7) Failed to connect"));
    assert!(can("the download of u failed: curl: (6) Could not resolve host"));
    assert!(can("the download of u failed: curl: (22) The requested URL returned error: 403"));
    assert!(can("the download of u failed: curl: (22) The requested URL returned error: 502"));
    assert!(can("the download took too long and was stopped"));
    assert!(!can("the download of u failed: curl: (22) The requested URL returned error: 400"));
    assert!(!can("the download of u failed: curl: (23) Failure writing output to destination"));
    assert!(!can("cancelled"));
    assert!(!can("could not run curl (x) — is curl installed?"));
}
