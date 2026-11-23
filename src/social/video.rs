//! Videos in a post (SOC-066, SOC-067): which files are videos, reading one for upload (a private
//! temporary copy, at most `MAX_VIDEO_BYTES`), and the way a video reaches Bluesky: through the
//! video service (`video.bsky.app`) with a service token, never the account's own session token
//! (SEC-027). Mastodon takes a video through `/api/v2/media` like a picture (`mastodon::upload`).

use super::actions::{ActionError, TempFile};
use super::bluesky::Api;
use super::*;
use crate::calendar::dav;
use serde_json::json;
use std::path::{Path, PathBuf};

/// The most bytes of a video Shale uploads, to any server (SEC-026): Bluesky takes 100 MB at most
/// and Mastodon servers name smaller limits.
pub const MAX_VIDEO_BYTES: u64 = 100_000_000;
/// What a Mastodon server that names no video limit gets.
pub const MASTODON_VIDEO_BYTES: u64 = 40 * 1024 * 1024;
/// The address of Bluesky's video service.
pub const VIDEO_SERVICE: &str = "https://video.bsky.app";
/// How often `getJobStatus` is asked for a video (5 minutes at the real pace).
const JOB_TRIES: u32 = 300;
/// The video types the file dialog offers.
pub const VIDEO_EXTENSIONS: [&str; 5] = ["mp4", "m4v", "mov", "webm", "mpeg"];

/// A video of a draft: the file and its description.
#[derive(Clone, Debug, PartialEq)]
pub struct DraftVideo {
    pub path: PathBuf,
    pub alt: String,
}

/// The type of a video file from its first bytes: (media type, extension). MP4 and QuickTime
/// have `ftyp` at byte 4, WebM (Matroska) starts with `1A 45 DF A3`, MPEG with `00 00 01 BA/B3`.
pub fn sniff(head: &[u8]) -> Option<(&'static str, &'static str)> {
    if head.len() >= 12 && &head[4..8] == b"ftyp" {
        return Some(if &head[8..12] == b"qt  " { ("video/quicktime", "mov") } else { ("video/mp4", "mp4") });
    }
    if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some(("video/webm", "webm"));
    }
    if head.starts_with(&[0, 0, 1, 0xBA]) || head.starts_with(&[0, 0, 1, 0xB3]) {
        return Some(("video/mpeg", "mpeg"));
    }
    None
}

/// A video ready for upload: a private temporary copy (SEC-026) that is removed when this is dropped.
#[derive(Debug)]
pub struct PreparedVideo {
    pub file: TempFile,
    pub mime: &'static str,
    /// the name the video service is told
    pub name: String,
}

/// Read a video file for upload: a readable MP4, QuickTime, WebM or MPEG file of at most `limit`
/// bytes (and never more than `MAX_VIDEO_BYTES`); refused with words otherwise.
pub fn prepare(path: &Path, limit: u64) -> Result<PreparedVideo, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "the file".into());
    let meta = std::fs::metadata(path).map_err(|e| format!("{name} cannot be read ({e})"))?;
    if !meta.is_file() {
        return Err(format!("{name} is not a file"));
    }
    let limit = limit.min(MAX_VIDEO_BYTES);
    let mb = |n: u64| format!("{:.1} MB", n as f64 / 1_000_000.0);
    if meta.len() > limit {
        return Err(format!("{name} is {}; videos can be up to {}", mb(meta.len()), mb(limit)));
    }
    let mut head = [0u8; 16];
    let n = crate::fsutil::open_read(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).map_err(|e| format!("{name} cannot be read ({e})"))?;
    let (mime, ext) = sniff(&head[..n]).ok_or_else(|| format!("{name} is not an MP4, QuickTime, WebM or MPEG video"))?;
    let file = TempFile::copy_of(path, ext)?;
    Ok(PreparedVideo { file, mime, name: format!("video.{ext}") })
}

fn aud_of(host: &str) -> String {
    format!("did:web:{}", host.replace(':', "%3A"))
}

/// A service token: the account's server vouches for the account towards `aud` for one method
/// (`com.atproto.server.getServiceAuth`). It is valid for half an hour and for that method only.
fn service_token(api: &mut Api, aud: &str, lxm: &str) -> Result<Secret, String> {
    let exp = (t::now() + 1800).to_string();
    let mut v = api.get("com.atproto.server.getServiceAuth", &[("aud", aud), ("lxm", lxm), ("exp", &exp)])?;
    let token = secret_of(&v["token"]);
    wipe_value(&mut v);
    if token.is_empty() {
        return Err("the server did not give a token for the video service".into());
    }
    Ok(token)
}

/// The `jobStatus` of an answer of the video service (the answer holds it, or is it).
fn job_of(v: &Value) -> &Value {
    if v["jobStatus"].is_object() {
        &v["jobStatus"]
    } else {
        v
    }
}

/// Put a video on Bluesky and return the `app.bsky.embed.video` for the post (SOC-067): ask the
/// video service whether this account may upload (`getUploadLimits`; its message is the answer
/// when not), upload the file with a service token for the account's server (`uploadVideo`), then
/// ask `getJobStatus` every `Api::video_poll` until the service has the blob (`JOB_TRIES` at most).
pub fn bluesky_embed(api: &mut Api, video: &DraftVideo) -> Result<Value, ActionError> {
    let prepared = prepare(&video.path, MAX_VIDEO_BYTES)?;
    let did = api.did()?;
    let pds = api.session.as_ref().map(|s| s.pds.clone()).ok_or("not signed in")?;
    let (service, poll, tries) = (api.video_service.clone(), api.video_poll, JOB_TRIES);
    let service_aud = aud_of(&dav::host_of(&service));
    // may this account upload now?
    let limits = {
        let token = service_token(api, &service_aud, "app.bsky.video.getUploadLimits")?;
        super::api("GET", &format!("{service}/xrpc/app.bsky.video.getUploadLimits"), Some(token.expose()), None).map_err(String::from)?
    };
    if limits["canUpload"].as_bool() == Some(false) {
        let why = text(&limits["message"]);
        return Err(format!("Bluesky does not take a video from this account now{}", if why.is_empty() { String::new() } else { format!(": {why}") }).into());
    }
    // the upload: the token is for the account's own server (the video service hands the file to it)
    let upload_token = service_token(api, &aud_of(&dav::host_of(&pds)), "com.atproto.repo.uploadBlob")?;
    let url = format!("{service}/xrpc/app.bsky.video.uploadVideo{}", query(&[("did", &did), ("name", &prepared.name)]));
    let answer = match super::api_upload("POST", &url, upload_token.expose(), Some(prepared.mime), dav::Upload::Raw { path: &prepared.file.0 }) {
        Ok(v) => v,
        // the same video was processed before: the job is named in the answer, and has the blob
        Err(e) if e.status == 409 && !text(&job_of(&e.body)["jobId"]).is_empty() => e.body,
        Err(e) => return Err(e.message.into()),
    };
    drop(prepared);
    let job = job_of(&answer);
    let job_id = text(&job["jobId"]);
    if job_id.is_empty() {
        return Err("the video service did not accept the video".into());
    }
    let status_token = service_token(api, &service_aud, "app.bsky.video.getJobStatus")?;
    let mut last = job.clone();
    for i in 0..tries {
        // done when it has the blob, or failed for good ("already exists" has failed too and names the blob: ask once)
        if last["blob"].is_object() || (text(&last["state"]) == "JOB_STATE_FAILED" && text(&last["error"]) != "already_exists") {
            break;
        }
        if i > 0 {
            std::thread::sleep(poll);
        }
        let v = super::api("GET", &format!("{service}/xrpc/app.bsky.video.getJobStatus{}", query(&[("jobId", &job_id)])), Some(status_token.expose()), None).map_err(String::from)?;
        last = job_of(&v).clone();
        if text(&last["state"]) == "JOB_STATE_COMPLETED" && !last["blob"].is_object() {
            break;
        }
    }
    // a blob is the answer whatever the state says (an "already exists" job has failed and has it)
    if !last["blob"].is_object() {
        let m = text(&last["message"]);
        return Err(match text(&last["state"]).as_str() {
            "JOB_STATE_FAILED" => format!("Bluesky could not process the video{}", if m.is_empty() { String::new() } else { format!(": {m}") }),
            "JOB_STATE_COMPLETED" => "the video service did not name the video it made".to_string(),
            _ => "Bluesky is still processing the video; try again in a few minutes".into(),
        }
        .into());
    }
    let mut embed = json!({"$type": "app.bsky.embed.video", "video": last["blob"].clone()});
    if !video.alt.trim().is_empty() {
        embed["alt"] = json!(video.alt.trim());
    }
    Ok(embed)
}
