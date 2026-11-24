//! A pretend GitHub for the tests of the updates (UPD): plain HTTP on 127.0.0.1 that serves the
//! newest release as JSON and the files it names, and counts the requests it gets.

use super::sshsig;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The seed of the key that signs the pretend releases; [`super::check::Source::pretend`] trusts it.
pub const SIGNER: u8 = 7;

/// A seed of a key that is not the one Source::pretend trusts.
pub const OTHER_SIGNER: u8 = 8;

/// How a pretend release is signed (UPD-022).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    /// Every file has a `.sig` made with the trusted key.
    Valid,
    /// No `.sig` at all: a release the owner has not signed yet, or an old one.
    Unsigned,
    /// The `.sig` is of the trusted key but of other bytes: the file was changed after signing
    /// (and its `.sha256` made to fit, as a thief of the account would).
    Tampered,
    /// Signed with a key that is not the trusted one.
    OtherKey,
    /// Signed with the trusted key, but for another file name (a file of an older release).
    OtherName,
}

pub struct FakeGithub {
    pub port: u16,
    routes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    hits: Arc<Mutex<Vec<String>>>,
    /// The whole head (request line and headers) of every request.
    heads: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

fn serve(mut s: TcpStream, routes: &Mutex<HashMap<String, Vec<u8>>>, hits: &Mutex<Vec<String>>, heads: &Mutex<Vec<String>>) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let mut raw = Vec::new();
    let mut buf = [0u8; 1024];
    while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
        }
    }
    let text = String::from_utf8_lossy(&raw).into_owned();
    let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
    hits.lock().unwrap().push(path.clone());
    heads.lock().unwrap().push(text.clone());
    let body = routes.lock().unwrap().get(&path).cloned();
    let _ = match body {
        Some(b) => {
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len());
            s.write_all(&b)
        }
        None => write!(s, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
    };
}

impl FakeGithub {
    pub fn start() -> FakeGithub {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let routes = Arc::new(Mutex::new(HashMap::new()));
        let hits = Arc::new(Mutex::new(Vec::new()));
        let heads = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (r, h, hd, sp) = (routes.clone(), hits.clone(), heads.clone(), stop.clone());
        let join = std::thread::spawn(move || {
            while !sp.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((s, _)) => {
                        let (r, h, hd) = (r.clone(), h.clone(), hd.clone());
                        std::thread::spawn(move || {
                            let _ = s.set_nonblocking(false);
                            serve(s, &r, &h, &hd);
                        });
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        FakeGithub { port, routes, hits, heads, stop, join: Some(join) }
    }

    /// The address to give [`super::check::Source::pretend`].
    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn serve(&self, path: &str, body: impl Into<Vec<u8>>) {
        self.routes.lock().unwrap().insert(path.to_string(), body.into());
    }

    /// The paths asked for so far.
    pub fn hits(&self) -> Vec<String> {
        self.hits.lock().unwrap().clone()
    }

    /// The head of every request so far, in order.
    pub fn heads(&self) -> Vec<String> {
        self.heads.lock().unwrap().clone()
    }

    /// Publish version `version` as the newest release with these files (name, bytes); each file
    /// gets its `.sha256` (`<hex>  <name>`), unless `damage_checksums` makes the hash wrong, and
    /// its valid signature (UPD-022).
    pub fn publish(&self, version: &str, files: &[(&str, Vec<u8>)], damage_checksums: bool) {
        self.publish_signed(version, files, damage_checksums, Sign::Valid);
    }

    /// [`FakeGithub::publish`] with the signature made as `sign` says.
    pub fn publish_signed(&self, version: &str, files: &[(&str, Vec<u8>)], damage_checksums: bool, sign: Sign) {
        let mut assets = vec![];
        for (name, bytes) in files {
            let hash = if damage_checksums { "0".repeat(64) } else { crate::pdf::tex::archive::sha256_hex(bytes) };
            self.serve(&format!("/dl/{name}"), bytes.clone());
            self.serve(&format!("/dl/{name}.sha256"), format!("{hash}  {name}\n"));
            assets.push(serde_json::json!({"name": name, "browser_download_url": format!("{}/dl/{name}", self.base()), "size": bytes.len()}));
            assets.push(serde_json::json!({"name": format!("{name}.sha256"), "browser_download_url": format!("{}/dl/{name}.sha256", self.base()), "size": 80}));
            let signature = match sign {
                Sign::Unsigned => None,
                Sign::Valid => Some(sshsig::testing::sign(SIGNER, &sshsig::namespace(name), bytes)),
                Sign::Tampered => Some(sshsig::testing::sign(SIGNER, &sshsig::namespace(name), b"what the owner signed")),
                Sign::OtherKey => Some(sshsig::testing::sign(OTHER_SIGNER, &sshsig::namespace(name), bytes)),
                Sign::OtherName => Some(sshsig::testing::sign(SIGNER, &sshsig::namespace("shale-0.0.1-linux-x86_64.tar.gz"), bytes)),
            };
            if let Some(text) = signature {
                self.serve(&format!("/dl/{name}.sig"), text);
                assets.push(serde_json::json!({"name": format!("{name}.sig"), "browser_download_url": format!("{}/dl/{name}.sig", self.base()), "size": 400}));
            }
        }
        let release = serde_json::json!({"tag_name": format!("v{version}"), "draft": false, "prerelease": false, "body": format!("Notes of {version}"), "assets": assets});
        self.serve("/repos/rabalyn/shale/releases/latest", release.to_string());
    }
}

impl Drop for FakeGithub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}
