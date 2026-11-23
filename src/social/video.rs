//! Videos in a post (SOC-066, SOC-067): which files are videos, reading one for upload (a private
//! temporary copy, at most `MAX_VIDEO_BYTES`), and the way a video reaches Bluesky: through the
//! video service (`video.bsky.app`) with a service token, never the account's own session token
//! (SEC-027). Mastodon takes a video through `/api/v2/media` like a picture (`mastodon::upload`).

use super::actions::{ActionError, TempFile};
use super::bluesky::Api;
use super::*;
use crate::calendar::dav;
use crate::media::ffmpeg;
use serde_json::json;
use std::path::{Path, PathBuf};

/// The most bytes of a video Shale uploads, to any server (SEC-026): Bluesky takes 100 MB at most
/// and Mastodon servers name smaller limits.
pub const MAX_VIDEO_BYTES: u64 = 100_000_000;
/// What a Mastodon server that names no video limit gets.
pub const MASTODON_VIDEO_BYTES: u64 = 40 * 1024 * 1024;
/// The address of Bluesky's video service.
pub const VIDEO_SERVICE: &str = "https://video.bsky.app";
/// How often `getJobStatus` is asked for a video (about 5 minutes at the real pace of 1.5 s).
const JOB_TRIES: u32 = 200;
/// The longest video Bluesky's own app lets you attach: `VIDEO_MAX_DURATION_MS` (10 minutes) in
/// `src/lib/constants.ts` of the Bluesky app's source (bluesky-social/social-app).
pub const BLUESKY_MAX_SECONDS: f64 = 600.0;
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

/// What ffprobe says about a video file; None without ffmpeg or when it cannot read the file (the
/// size and length are then left to the service).
pub fn probe_file(path: &Path) -> Option<ffmpeg::Info> {
    probe_with(ffmpeg::locate().as_ref(), path)
}

/// `probe_file` with the tools given (None: there are none).
pub fn probe_with(tools: Option<&ffmpeg::Tools>, path: &Path) -> Option<ffmpeg::Info> {
    tools?.probe(path).ok()
}

/// Why the service refused a job, in words: the validation errors and failure codes of the job
/// status, as the messages of Bluesky's own app (`src/view/com/composer/state/video.ts`).
fn failure_words(job: &Value) -> String {
    let (error, message) = (text(&job["error"]), text(&job["message"]));
    let words = match (error.as_str(), text(&job["failureCode"]).as_str()) {
        ("video_too_long", _) => "the video is too long".to_string(),
        ("bad_aspect_ratio", _) => "the video has an unsupported aspect ratio".to_string(),
        ("unsupported_codec", _) => "the video uses an unsupported format".to_string(),
        ("encoded_video_too_large", _) => "the processed video is too large".to_string(),
        (_, "pds_upload_unsupported_blob_size") => "your server does not take videos this large".to_string(),
        _ if !error.is_empty() => error,
        _ if !message.is_empty() => message,
        _ => String::new(),
    };
    format!("Bluesky could not process the video{}", if words.is_empty() { String::new() } else { format!(": {words}") })
}

/// Put a video on Bluesky and return the `app.bsky.embed.video` for the post (SOC-067), the way
/// Bluesky's own app does it (`src/lib/media/video/upload.shared.ts`, `upload.ts` and
/// `src/view/com/composer/state/video.ts` of bluesky-social/social-app): a video the file says is
/// longer than 10 minutes is refused first; then `getUploadLimits` is asked with a service token
/// for the video service (`aud` `did:web:video.bsky.app`, `lxm` `app.bsky.video.getUploadLimits`;
/// its message is the answer when `canUpload` is false); the file is uploaded with a service token
/// for the account's server (`aud` `did:web:<its host>`, `lxm` `com.atproto.repo.uploadBlob`);
/// then `getJobStatus`, which needs no token, is asked every `Api::video_poll` until the job is
/// `JOB_STATE_COMPLETED` with its blob or `JOB_STATE_FAILED` with its error.
pub fn bluesky_embed(api: &mut Api, video: &DraftVideo) -> Result<Value, ActionError> {
    let prepared = prepare(&video.path, MAX_VIDEO_BYTES)?;
    // the size and the length from ffprobe, when it is there
    let info = (api.probe)(&video.path);
    if let Some(secs) = info.as_ref().and_then(|i| i.duration).filter(|d| *d > BLUESKY_MAX_SECONDS) {
        let name = video.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        return Err(format!("{name} is {:.0} minutes long; Bluesky takes videos up to {:.0} minutes.", secs / 60.0, BLUESKY_MAX_SECONDS / 60.0).into());
    }
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
    let job_id = text(&job_of(&answer)["jobId"]);
    if job_id.is_empty() {
        return Err("the video service did not accept the video".into());
    }
    let mut last = job_of(&answer).clone();
    let mut failures = 0;
    for i in 0..tries {
        // done when it has the blob, or failed for good ("already exists" has failed too and names the blob: ask once)
        if last["blob"].is_object() || (text(&last["state"]) == "JOB_STATE_FAILED" && text(&last["error"]) != "already_exists") {
            break;
        }
        if i > 0 {
            std::thread::sleep(poll);
        }
        // the status of a job is public: no token goes with this request
        match super::api("GET", &format!("{service}/xrpc/app.bsky.video.getJobStatus{}", query(&[("jobId", &job_id)])), None, None) {
            Ok(v) => {
                failures = 0;
                last = job_of(&v).clone();
                if text(&last["state"]) == "JOB_STATE_COMPLETED" && !last["blob"].is_object() {
                    break;
                }
            }
            // a few failed looks do not end the wait (the app allows 50)
            Err(_) if failures < 5 => failures += 1,
            Err(e) => return Err(e.message.into()),
        }
    }
    // a blob is the answer whatever the state says (an "already exists" job has failed and has it)
    if !last["blob"].is_object() {
        return Err(match text(&last["state"]).as_str() {
            "JOB_STATE_FAILED" => failure_words(&last),
            "JOB_STATE_COMPLETED" => "Bluesky finished the video but did not name it".to_string(),
            _ => "Bluesky is still processing the video; try again in a few minutes".into(),
        }
        .into());
    }
    let mut embed = json!({"$type": "app.bsky.embed.video", "video": last["blob"].clone()});
    // the size of the picture (as the file shows it), so a vertical video is not drawn with bars
    if let Some(i) = info.filter(|i| i.width > 0 && i.height > 0) {
        embed["aspectRatio"] = json!({"width": i.width, "height": i.height});
    }
    if !video.alt.trim().is_empty() {
        embed["alt"] = json!(video.alt.trim());
    }
    Ok(embed)
}
