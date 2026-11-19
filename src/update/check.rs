//! Looking for a newer release (UPD-003 to UPD-006): the request to GitHub, the release it
//! answers with, the comparison of versions, the choice of the file for this computer, and the
//! download of that file into a staging folder, verified against its `.sha256` (UPD-007).
//! Everything that touches the network goes through [`crate::download`] (`curl`).

use crate::download::{self, Request};
use crate::pdf::tex::archive::sha256_hex;
use serde::Deserialize;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// The API of GitHub; the repository of Shale is [`REPOSITORY`].
pub const API: &str = "https://api.github.com";
pub const REPOSITORY: &str = "rabalyn/shale";

/// The most bytes of the release description from the API, of a checksum file and of a download.
const MAX_API: u64 = 4 << 20;
const MAX_SUM: u64 = 64 << 10;
const MAX_ASSET: u64 = 400 << 20;
/// The most characters of the release notes that are kept.
const MAX_NOTES: usize = 20_000;

/// Where releases are asked for. Production: GitHub, https only; tests: a pretend server on
/// `127.0.0.1` (there is no setting for it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    api: String,
    allow_http: bool,
}

impl Source {
    pub fn github() -> Source {
        Source { api: API.to_string(), allow_http: false }
    }

    /// Tests only: a pretend GitHub at `base` (`http://127.0.0.1:port`).
    #[cfg(all(test, unix))]
    pub fn pretend(base: &str) -> Source {
        Source { api: base.trim_end_matches('/').to_string(), allow_http: true }
    }

    /// The address of the newest release.
    pub fn latest_url(&self) -> String {
        format!("{}/repos/{REPOSITORY}/releases/latest", self.api)
    }

    /// Whether a download address of a release may be used: on GitHub only the release files of
    /// the repository (https); a pretend server in a test may hand out its own.
    pub fn accepts(&self, url: &str) -> bool {
        if self.allow_http {
            return url.starts_with("http://127.0.0.1:") || url.starts_with("https://");
        }
        url.starts_with(&format!("https://github.com/{REPOSITORY}/releases/download/")) && !url.chars().any(|c| c.is_whitespace() || c.is_control())
    }
}

// ---------------------------------------------------------------- versions

/// One dot-separated part of a pre-release (`rc`, `1`).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Number(u64),
    Text(String),
}

impl Ord for Part {
    fn cmp(&self, other: &Part) -> Ordering {
        match (self, other) {
            (Part::Number(a), Part::Number(b)) => a.cmp(b),
            (Part::Text(a), Part::Text(b)) => a.cmp(b),
            // semver: a number is lower than text
            (Part::Number(_), Part::Text(_)) => Ordering::Less,
            (Part::Text(_), Part::Number(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for Part {
    fn partial_cmp(&self, other: &Part) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A semantic version (`0.6.7`, `1.0.0-rc.1`, a leading `v` allowed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    core: [u64; 3],
    pre: Vec<Part>,
}

impl Version {
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        // build metadata does not count
        let text = text.split('+').next()?;
        let (core, pre) = match text.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (text, None),
        };
        let mut nums = core.split('.').map(|n| if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) { n.parse::<u64>().ok() } else { None });
        let version = [nums.next()??, nums.next()??, nums.next()??];
        if nums.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => vec![],
            Some(p) => p
                .split('.')
                .map(|s| {
                    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                        None
                    } else if s.bytes().all(|b| b.is_ascii_digit()) {
                        s.parse().ok().map(Part::Number)
                    } else {
                        Some(Part::Text(s.to_string()))
                    }
                })
                .collect::<Option<Vec<_>>>()?,
        };
        Some(Version { core: version, pre })
    }

    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Version) -> Ordering {
        self.core.cmp(&other.core).then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            // a release is newer than its own pre-releases
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => self.pre.cmp(&other.pre),
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Version) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Whether `latest` is a version newer than `current`; a text that is no version is never newer.
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (Version::parse(current), Version::parse(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

// ---------------------------------------------------------------- the release

/// A file of a release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

/// A published release: its version (`0.6.7`, without the `v`), its notes and its files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

/// The release in the answer of the API. `None` for a draft, a pre-release, or a tag that is
/// not a plain version: those are never offered. An answer that is no release is an error.
pub fn parse_release(json: &str) -> Result<Option<Release>, String> {
    let raw: RawRelease = serde_json::from_str(json).map_err(|e| format!("GitHub's answer is not a release ({e})"))?;
    if raw.draft || raw.prerelease {
        return Ok(None);
    }
    if !Version::parse(&raw.tag_name).is_some_and(|v| !v.is_prerelease()) {
        return Ok(None);
    }
    let version = raw.tag_name.trim().trim_start_matches('v').to_string();
    let notes: String = raw.body.unwrap_or_default().chars().take(MAX_NOTES).collect();
    let assets = raw.assets.into_iter().map(|a| Asset { name: a.name, url: a.browser_download_url, size: a.size }).collect();
    Ok(Some(Release { version, notes, assets }))
}

/// The name of the download for a system (as `.github/workflows/build.yml` names them), or none
/// when there is no build for it.
pub fn asset_name(version: &str, os: &str, arch: &str) -> Option<String> {
    let (system, ext) = match (os, arch) {
        ("linux", "x86_64") => ("linux-x86_64", "tar.gz"),
        ("macos", "aarch64") => ("macos-arm64", "dmg"),
        ("windows", "x86_64") => ("windows-x86_64", "zip"),
        _ => return None,
    };
    Some(format!("shale-{version}-{system}.{ext}"))
}

/// The download of this system and its checksum file, or what is missing in words.
pub fn asset_for<'a>(release: &'a Release, os: &str, arch: &str) -> Result<(&'a Asset, &'a Asset), String> {
    let name = asset_name(&release.version, os, arch).ok_or_else(|| format!("there is no download of Shale {} for {os} on {arch}", release.version))?;
    let find = |n: &str| release.assets.iter().find(|a| a.name == n);
    let file = find(&name).ok_or_else(|| format!("the release {} has no file {name}", release.version))?;
    let sum = find(&format!("{name}.sha256")).ok_or_else(|| format!("the release {} has no checksum file for {name}, so it is not installed", release.version))?;
    Ok((file, sum))
}

/// The hash in a `.sha256` file (`<hex>  <name>`, as `sha256sum` and `shasum -a 256` write it),
/// which must name `name`.
pub fn parse_checksum(text: &str, name: &str) -> Result<String, String> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).ok_or("the checksum file is empty")?;
    let mut words = line.split_whitespace();
    let hash = words.next().unwrap_or("").to_ascii_lowercase();
    let file = words.next().map(|f| f.trim_start_matches('*'));
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("the checksum file does not hold a SHA-256 hash".into());
    }
    match file {
        Some(f) if f != name => Err(format!("the checksum file is for {f}, not for {name}")),
        _ => Ok(hash),
    }
}

// ---------------------------------------------------------------- asking GitHub

/// The newest release as GitHub tells it (`None`: nothing that may be offered).
pub fn fetch_latest(source: &Source, cancel: &AtomicBool) -> Result<Option<Release>, String> {
    let _flow = tracing::info_span!("flow.update_check", peer = "app").entered();
    let dir = std::env::temp_dir().join(format!("shale-update-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
    std::fs::create_dir_all(&dir).map_err(|e| format!("no temporary folder ({e})"))?;
    let file = dir.join("latest.json");
    let url = source.latest_url();
    let request = Request { url: &url, max_bytes: MAX_API, timeout: Duration::from_secs(60), allow_http: source.allow_http, headers: &["Accept: application/vnd.github+json", "X-GitHub-Api-Version: 2022-11-28"] };
    let got = tracing::info_span!("flow.update_check.request", peer = "github")
        .in_scope(|| download::to_file(cancel, &request, &file))
        .and_then(|()| tracing::info_span!("flow.update_check.read_answer", peer = "disk").in_scope(|| crate::fsutil::read_to_string(&file)).map_err(|e| format!("the answer could not be read ({e})")));
    let _ = std::fs::remove_dir_all(&dir);
    parse_release(&got?)
}

// ---------------------------------------------------------------- the download

/// A release file that was downloaded and whose checksum matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Staged {
    pub version: String,
    /// The folder of this version in the staging area.
    pub dir: PathBuf,
    pub archive: PathBuf,
}

/// Where the downloads of updates are kept: `<cache>/shale/updates`.
pub fn staging_root() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("shale").join("updates")
}

/// Whether `archive` has the SHA-256 that `sum_file` names for `name`; a file that does not is deleted.
pub fn verify(archive: &Path, sum_file: &Path, name: &str) -> Result<(), String> {
    let expected = crate::fsutil::read_to_string(sum_file).map_err(|e| format!("the checksum file could not be read ({e})")).and_then(|t| parse_checksum(&t, name));
    let bytes = crate::fsutil::read(archive).map_err(|e| format!("the download could not be read ({e})"));
    let result = match (expected, bytes) {
        (Ok(want), Ok(bytes)) => {
            let got = sha256_hex(&bytes);
            if got == want {
                Ok(())
            } else {
                Err(format!("{name} is damaged: its SHA-256 is {got}, the release says {want}. It was deleted and nothing was installed."))
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(format!("{e}. Nothing was installed.")),
    };
    if result.is_err() {
        let _ = std::fs::remove_file(archive);
        let _ = std::fs::remove_file(sum_file);
    }
    result
}

/// Download the file of `release` for this system and its checksum into `<root>/<version>/` and
/// verify it. A file that is already there and matches is not downloaded again.
pub fn stage(source: &Source, release: &Release, os: &str, arch: &str, root: &Path, cancel: &AtomicBool) -> Result<Staged, String> {
    let _flow = tracing::info_span!("flow.update_download", peer = "app").entered();
    let (file, sum) = asset_for(release, os, arch)?;
    for a in [file, sum] {
        if !source.accepts(&a.url) {
            return Err(format!("the release names the address {:?} for {}, which is not a download of Shale: refused", a.url, a.name));
        }
    }
    let dir = tracing::info_span!("flow.update_download.prepare", peer = "disk").in_scope(|| {
        clear_other_versions(root, &release.version);
        let dir = root.join(&release.version);
        std::fs::create_dir_all(&dir).map(|()| dir.clone()).map_err(|e| format!("{} could not be made ({e})", dir.display()))
    })?;
    let (archive, sum_file) = (dir.join(&file.name), dir.join(&sum.name));
    let get = |a: &Asset, to: &Path, max: u64, secs: u64| download::to_file(cancel, &Request { url: &a.url, max_bytes: max, timeout: Duration::from_secs(secs), allow_http: source.allow_http, headers: &[] }, to);
    // the checksum is always fetched anew: a download that was kept is checked against the current one
    tracing::info_span!("flow.update_download.checksum_download", peer = "github").in_scope(|| get(sum, &sum_file, MAX_SUM, 120))?;
    if !(archive.is_file() && verify_quiet(&archive, &sum_file, &file.name)) {
        let _ = std::fs::remove_file(&archive);
        tracing::info_span!("flow.update_download.archive_download", peer = "github").in_scope(|| get(file, &archive, MAX_ASSET, 30 * 60))?;
        tracing::info_span!("flow.update_download.verify", peer = "disk").in_scope(|| verify(&archive, &sum_file, &file.name))?;
    }
    Ok(Staged { version: release.version.clone(), dir, archive })
}

/// [`verify`] for a file that may legitimately not match (a leftover): nothing is deleted.
fn verify_quiet(archive: &Path, sum_file: &Path, name: &str) -> bool {
    let (Ok(text), Ok(bytes)) = (crate::fsutil::read_to_string(sum_file), crate::fsutil::read(archive)) else { return false };
    parse_checksum(&text, name).is_ok_and(|want| want == sha256_hex(&bytes))
}

/// The folders of other versions in the staging area, which an update that went through can remove.
pub fn clear_other_versions(root: &Path, keep: &str) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for e in entries.flatten() {
        if e.file_name().to_string_lossy() != keep && e.path().is_dir() {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}
