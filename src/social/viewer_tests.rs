//! The viewer with videos and GIFs (SOC-076, MEDIA-025): stepping across pictures and videos,
//! the spinner and the error of a clip, the player that starts, pauses with Space and stops
//! when the viewer steps away or closes, a GIF that is muted and loops, and the message of a
//! machine without ffmpeg. Tests that need the real ffmpeg skip with a note when it is not
//! installed (BUILD-018). Nothing here waits a fixed number of frames: each wait is on the state
//! it asserts.

use super::clips::{Clips, Source};
use super::feed_ui::tests::{at, demo_state, has, H};
use super::state::SocialState;
use super::{Media, MediaKind, SocialAccountEntry};
use crate::media::ffmpeg::Tools;
use eframe::egui;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const VIDEO: &str = "https://v.test/video-564.mp4";
const GIF: &str = "https://v.test/gif-564.mp4";

fn media(kind: MediaKind, url: &str, alt: &str) -> Media {
    Media { kind, url: url.into(), preview_url: String::new(), alt: alt.into(), file_name_hint: url.rsplit('/').next().unwrap_or("x").into(), blob_cid: None }
}

/// The demo feed with a post of a picture, a video and a GIF (post 3 of the feed).
fn state_with_clips() -> (SocialState, Vec<SocialAccountEntry>) {
    let (mut st, accounts) = demo_state();
    for t in st.timelines.values_mut() {
        let p = &mut t.posts[3];
        let mut video = media(MediaKind::Video, VIDEO, "A short film of nothing");
        video.preview_url = p.media[0].url.clone();
        let mut gif = media(MediaKind::Gifv, GIF, "");
        gif.preview_url = p.media[0].url.clone();
        p.media = vec![p.media[0].clone(), video, gif];
    }
    (st, accounts)
}

fn fake_ffmpeg() -> Option<Tools> {
    Some(Tools { ffmpeg: PathBuf::from("ffmpeg"), ffprobe: PathBuf::from("ffprobe") })
}

fn no_ffmpeg() -> Option<Tools> {
    None
}

/// Frames until `ok` holds (the work is on threads), at most 15 seconds.
fn until(h: &mut H, st: &mut SocialState, accounts: &[SocialAccountEntry], what: &str, mut ok: impl FnMut(&H, &SocialState) -> bool) {
    let end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < end {
        h.frame(vec![], st, accounts);
        if ok(h, st) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{what}: {:?}", st.rects.iter().map(|r| &r.0).collect::<Vec<_>>());
}

fn have_ffmpeg() -> bool {
    let found = crate::media::ffmpeg::locate().is_some();
    if !found {
        println!("skipping: ffmpeg and ffprobe are not installed");
    }
    found
}

/// Put a clip of the media fixtures where the cache keeps the file of `url`.
fn seed(st: &SocialState, url: &str, fixture: &str) -> PathBuf {
    let clips_dir = st.cache_dir.clone();
    std::fs::create_dir_all(&clips_dir).unwrap();
    let to = Clips::file_for(&clips_dir, &Source::Url(url.into()), "mp4");
    std::fs::copy(crate::media::testdata::fixture(fixture), &to).unwrap();
    to
}

fn current(h: &H) -> Option<PathBuf> {
    h.tab.media.current()
}

// spec: SOC-076
#[test]
fn soc_076_the_viewer_steps_across_pictures_videos_and_gifs_and_a_video_without_ffmpeg_shows_its_preview_and_says_why() {
    let (mut st, accounts) = state_with_clips();
    let mut h = H::new();
    h.tab.media.use_ffmpeg(no_ffmpeg);
    h.frame(vec![], &mut st, &accounts);
    assert!(has(&st, "media:3:1") && has(&st, "media:3:2"), "the previews of the video and the GIF can be clicked");
    h.click("media:3:0", &mut st, &accounts);
    let v = st.viewer.as_ref().unwrap();
    assert_eq!((v.items.len(), v.index), (3, 0));
    assert!(v.items[0].video.is_none() && v.items[1].video.is_some() && v.items[2].video.as_ref().unwrap().gif);
    h.wait_for("viewer:image", &mut st, &accounts);
    assert!(!has(&st, "viewer:no-ffmpeg"));
    // Right: the video; no ffmpeg: its preview picture, the reason, and nothing plays
    h.key(egui::Key::ArrowRight, &mut st, &accounts);
    assert_eq!(at(&st), Some(1));
    h.wait_for("viewer:image", &mut st, &accounts);
    assert!(has(&st, "viewer:no-ffmpeg") && has(&st, "viewer:browser") && !has(&st, "viewer:video"));
    assert!(has(&st, "viewer:alt"), "its description is below");
    assert_eq!(st.viewer.as_ref().unwrap().items[1].alt, "A short film of nothing");
    assert_eq!(st.viewer.as_ref().unwrap().items[1].url, VIDEO, "Open in browser opens the file");
    assert_eq!(current(&h), None);
    // the GIF, then back across the picture; the ends stop
    h.key(egui::Key::ArrowRight, &mut st, &accounts);
    assert_eq!(at(&st), Some(2));
    h.key(egui::Key::ArrowRight, &mut st, &accounts);
    assert_eq!(at(&st), Some(2));
    h.key(egui::Key::ArrowLeft, &mut st, &accounts);
    h.key(egui::Key::ArrowLeft, &mut st, &accounts);
    assert_eq!(at(&st), Some(0));
    h.wait_for("viewer:image", &mut st, &accounts);
    assert!(!has(&st, "viewer:no-ffmpeg"), "a picture has no such line");
    h.key(egui::Key::Escape, &mut st, &accounts);
    assert!(st.viewer.is_none());
    // a click on the preview of the video opens the viewer at the video
    h.click("media:3:1", &mut st, &accounts);
    assert_eq!(at(&st), Some(1));
    h.click("viewer:close", &mut st, &accounts);
    assert!(st.viewer.is_none());
}

// spec: SOC-076
#[test]
fn soc_076_a_video_on_its_way_shows_a_spinner_and_one_that_fails_an_error_and_stepping_goes_on() {
    let (mut st, accounts) = state_with_clips();
    let bad = "http://example.invalid/bad.mp4";
    for t in st.timelines.values_mut() {
        t.posts[3].media[1].url = "https://slow.test/slow-564.mp4".into();
        t.posts[3].media[2].url = bad.into();
    }
    let mut h = H::new();
    h.tab.media.use_ffmpeg(fake_ffmpeg);
    h.frame(vec![], &mut st, &accounts);
    st.clips.as_ref().unwrap().hold(&Source::Url("https://slow.test/slow-564.mp4".into()));
    h.click("media:3:1", &mut st, &accounts);
    assert_eq!(at(&st), Some(1));
    assert!(has(&st, "viewer:spinner") && !has(&st, "viewer:video") && !has(&st, "viewer:error"));
    assert!(has(&st, "viewer:alt"));
    // the next one is refused (not https): a message, the viewer still steps and closes
    h.click("viewer:next", &mut st, &accounts);
    h.wait_for("viewer:error", &mut st, &accounts);
    assert!(!has(&st, "viewer:spinner"));
    h.click("viewer:prev", &mut st, &accounts);
    assert!(has(&st, "viewer:spinner"));
    assert_eq!(current(&h), None, "nothing plays while nothing is here");
    h.key(egui::Key::Escape, &mut st, &accounts);
    assert!(st.viewer.is_none());
}

// spec: SOC-076, MEDIA-025
#[test]
fn soc_076_a_video_plays_when_opened_pauses_with_space_and_stops_when_the_viewer_steps_away_or_closes() {
    if !have_ffmpeg() {
        return;
    }
    let (mut st, accounts) = state_with_clips();
    let clip = seed(&st, VIDEO, "clip.mp4");
    let mut h = H::new();
    h.tab.media.use_silent_output();
    h.frame(vec![], &mut st, &accounts);
    h.click("media:3:1", &mut st, &accounts);
    assert_eq!(at(&st), Some(1));
    h.wait_for("viewer:video", &mut st, &accounts);
    let card = st.rects.iter().find(|r| r.0 == "viewer:video").unwrap().1;
    assert!(card.width() > 200.0 && card.width() <= 1000.0, "{card:?}");
    until(&mut h, &mut st, &accounts, "the video is loaded and runs", |h, _| h.tab.media.position().is_some_and(|p| p > 0.1));
    assert_eq!(current(&h), Some(clip.clone()));
    let view = h.tab.media.view(&h.ctx, &clip);
    assert!(view.playing && !view.muted, "a video starts with its sound");
    // Space pauses, Space plays again
    h.key(egui::Key::Space, &mut st, &accounts);
    h.frame(vec![], &mut st, &accounts);
    assert!(!h.tab.media.view(&h.ctx, &clip).playing);
    // the sound fades out within a few milliseconds: look after that
    std::thread::sleep(Duration::from_millis(100));
    let at_pause = h.tab.media.position().unwrap();
    std::thread::sleep(Duration::from_millis(150));
    h.frame(vec![], &mut st, &accounts);
    assert!((h.tab.media.position().unwrap() - at_pause).abs() < 0.001, "paused");
    h.key(egui::Key::Space, &mut st, &accounts);
    until(&mut h, &mut st, &accounts, "it plays on", |h, _| h.tab.media.position().is_some_and(|p| p > at_pause + 0.05));
    // with a dialog open the keys are not the viewer's: Space does nothing
    h.tab.keys = false;
    h.key(egui::Key::Space, &mut st, &accounts);
    h.key(egui::Key::Escape, &mut st, &accounts);
    h.tab.keys = true;
    assert!(st.viewer.is_some() && h.tab.media.view(&h.ctx, &clip).playing);
    // a step away stops it, and a step back starts it again
    h.key(egui::Key::ArrowLeft, &mut st, &accounts);
    assert_eq!(current(&h), None, "stepped to the picture: the video is stopped and freed");
    h.key(egui::Key::ArrowRight, &mut st, &accounts);
    until(&mut h, &mut st, &accounts, "the video starts again", |h, _| h.tab.media.position().is_some_and(|p| p > 0.05));
    assert_eq!(current(&h), Some(clip));
    // Esc closes the viewer and stops it
    h.key(egui::Key::Escape, &mut st, &accounts);
    assert!(st.viewer.is_none());
    assert_eq!(current(&h), None);
    // so does a viewer that is dropped some other way (another tab, a new vault)
    h.click("media:3:1", &mut st, &accounts);
    until(&mut h, &mut st, &accounts, "it runs", |h, _| h.tab.media.position().is_some());
    st.viewer = None;
    assert_eq!(current(&h), None);
}

// spec: SOC-076, MEDIA-025
#[test]
fn soc_076_a_gif_plays_muted_and_loops_and_the_person_can_unmute_it() {
    if !have_ffmpeg() {
        return;
    }
    let (mut st, accounts) = state_with_clips();
    let gif = seed(&st, GIF, "clip.mp4");
    let mut h = H::new();
    h.tab.media.use_silent_output();
    h.frame(vec![], &mut st, &accounts);
    h.click("media:3:2", &mut st, &accounts);
    assert_eq!(at(&st), Some(2));
    h.wait_for("viewer:video", &mut st, &accounts);
    until(&mut h, &mut st, &accounts, "the GIF runs", |h, _| h.tab.media.position().is_some_and(|p| p > 0.3));
    let view = h.tab.media.view(&h.ctx, &gif);
    assert!(view.playing && view.muted, "a GIF plays by itself and without sound");
    assert_eq!(h.tab.media.take_volume_change(), None, "muting a GIF is not the person's volume setting");
    // it passes the end of the one second clip and plays on from the start
    until(&mut h, &mut st, &accounts, "it runs to the end", |h, _| h.tab.media.position().is_some_and(|p| p > 0.8));
    until(&mut h, &mut st, &accounts, "it starts over", |h, _| h.tab.media.position().is_some_and(|p| p < 0.5));
    assert!(h.tab.media.view(&h.ctx, &gif).playing, "a GIF does not stop at the end");
    // the mute button of the player asks for the sound
    h.tab.media.set_muted(false);
    assert!(!h.tab.media.view(&h.ctx, &gif).muted);
    h.key(egui::Key::Escape, &mut st, &accounts);
    assert_eq!(current(&h), None);
}
