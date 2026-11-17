//! The video player driven in the whole window (MEDIA-001, MEDIA-007, MEDIA-009, MEDIA-010), like
//! the sound player in the tests above it. Tests that need the real ffmpeg skip with a note when
//! it is not installed (BUILD-018).

use super::{Fx, SettingsTab};
use crate::media::widget::BUTTON_RADIUS;
use eframe::egui;
use egui::{PointerButton, Pos2};
use std::path::Path;
use std::time::Duration;

const VIDEO_NOTE: &str = "# Watch\n\n![[clip.mp4]]\n\nafter the video\n";

/// The tools of this machine, or a note that the test is skipped (BUILD-018).
fn have_ffmpeg() -> bool {
    let found = crate::media::ffmpeg::locate().is_some();
    if !found {
        println!("skipping: ffmpeg and ffprobe are not installed");
    }
    found
}

fn texts(fx: &Fx) -> Vec<String> {
    fx.hl.text.iter().map(|(t, _)| t.clone()).collect()
}

// spec: MEDIA-001, MEDIA-009
#[test]
fn a_video_is_found_by_its_extension_in_any_case_and_the_player_shows_the_first_frame() {
    for name in ["a.mp4", "b.M4V", "c.Mov", "d.WEBM", "e.mkv"] {
        assert!(crate::media::is_video(Path::new(name)) && crate::media::is_media(Path::new(name)), "{name}");
        assert!(!crate::media::is_audio(Path::new(name)), "{name}");
    }
    for name in ["a.mp3", "b.avi", "c.mp4.txt", "d"] {
        assert!(!crate::media::is_video(Path::new(name)), "{name}");
    }
    if !have_ffmpeg() {
        return;
    }
    let mut fx = Fx::new("v-look", VIDEO_NOTE, &["clip.mp4"]);
    fx.reading(true);
    fx.label("Play");
    fx.label("Position");
    let abs = fx.dir.join("clip.mp4");
    fx.until("the first frame is read", |a| a.media.thumbnail_size(&abs).is_some());
    assert_eq!(fx.app.media.thumbnail_size(&abs), Some([160, 90]));
    fx.hl.frames(&mut fx.app, 3);
    assert!(texts(&fx).iter().any(|t| t == "0:01"), "the length shows: {:?}", texts(&fx));
    assert_eq!(fx.app.media.current(), None, "nothing starts by itself");
}

// spec: MEDIA-001
#[test]
fn the_video_player_is_in_the_editor_and_in_the_markdown_preview_as_well() {
    if !have_ffmpeg() {
        return;
    }
    let mut fx = Fx::new("v-editor", VIDEO_NOTE, &["clip.mp4"]);
    fx.reading(false);
    fx.label("Play");
    fx.app.cfg.live_preview = true;
    fx.hl.frames(&mut fx.app, 4);
    fx.label("Play");
}

// spec: MEDIA-009
#[test]
fn playing_a_video_shows_its_frames_pauses_and_seeks_with_the_bar() {
    if !have_ffmpeg() {
        return;
    }
    for clip in ["clip.mp4", "clip-silent.mp4"] {
        let mut fx = Fx::new("v-play", &format!("![[{clip}]]\n"), &[clip]);
        fx.reading(true);
        let abs = fx.dir.join(clip);
        // the length must be known, or the bar ignores a click
        fx.until("the look is done", |a| a.media.thumbnail_size(&abs).is_some());
        // start ffmpeg paused: the silent clip's 1 s clock must not run while a slow ffmpeg
        // (a loaded machine) has no picture yet, or the clip ends before the test looks
        fx.click_label("Play");
        fx.click_label("Pause");
        fx.until("the output is open", |a| a.media.position().is_some());
        fx.until("a frame of the video is shown", |a| a.media.picture_size().is_some());
        fx.click_label("Play"); // the clock starts only now
        assert_eq!(fx.app.media.picture_size(), Some([160, 90]), "{clip}");
        fx.until("the video runs", |a| a.media.position().is_some_and(|p| p > 0.15));
        fx.click_label("Pause");
        fx.hl.frames(&mut fx.app, 3);
        std::thread::sleep(Duration::from_millis(80));
        let p = fx.app.media.position().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!((fx.app.media.position().unwrap() - p).abs() < 0.001, "{clip}: paused");
        // a click on the bar restarts ffmpeg at that place
        let bar = fx.label("Position");
        fx.hl.click(&mut fx.app, Pos2::new(bar.left() + bar.width() * 0.9, bar.center().y), PointerButton::Primary);
        fx.until("the seek is done", |a| !a.media.seek_pending() && a.media.position().is_some_and(|p| p > 0.75));
        fx.label("Play");
    }
}

// spec: MEDIA-007
#[test]
fn only_one_of_a_video_and_a_sound_plays() {
    if !have_ffmpeg() {
        return;
    }
    let mut fx = Fx::new("v-one", "![[clip.mp4]]\n\n![[tone.wav]]\n", &["clip.mp4", "tone.wav"]);
    fx.reading(true);
    fx.play();
    let first = fx.app.media.current().expect("one plays");
    // that player's button says "Pause" now: the label "Play" is the other one's
    fx.click_label("Play");
    let second = fx.app.media.current().expect("the other plays");
    assert_ne!(first, second, "starting one stops the other");
    assert!([&first, &second].contains(&&fx.dir.join("clip.mp4")) && [&first, &second].contains(&&fx.dir.join("tone.wav")));
}

// spec: MEDIA-009
#[test]
fn a_video_that_is_damaged_says_so() {
    if !have_ffmpeg() {
        return;
    }
    let mut fx = Fx::new("v-bad", "![[bad.mp4]]\n", &["bad.mp4"]);
    fx.reading(true);
    std::thread::sleep(Duration::from_millis(1000));
    fx.hl.frames(&mut fx.app, 4);
    assert!(texts(&fx).iter().any(|t| t.contains("damaged")), "{:?}", texts(&fx));
}

/// The files the pretend system player was asked to open.
fn pretend_player() -> (crate::opener::BrowserGuard, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let opened = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = opened.clone();
    let guard = crate::opener::install_test_browser(Box::new(move |t| sink.lock().unwrap().push(t.to_string())));
    (guard, opened)
}

// spec: MEDIA-010
#[test]
fn without_ffmpeg_a_click_on_the_play_mark_or_the_tile_opens_the_system_player() {
    let (_guard, opened) = pretend_player();
    let mut fx = Fx::new_with("v-none", VIDEO_NOTE, &["clip.mp4"], |a| a.media.use_ffmpeg(|| None));
    fx.reading(true);
    std::thread::sleep(Duration::from_millis(300));
    fx.hl.frames(&mut fx.app, 4);
    assert!(texts(&fx).iter().any(|t| t.contains("ffmpeg is not installed")), "{:?}", texts(&fx));
    let button = fx.label("Play");
    fx.click_label("Play");
    fx.hl.frames(&mut fx.app, 3);
    assert_eq!(fx.app.media.current(), None, "nothing plays inside Shale");
    {
        let o = opened.lock().unwrap();
        assert_eq!(o.len(), 1, "{o:?}");
        assert!(o[0].ends_with("clip.mp4"), "{o:?}");
    }
    // the tile beside the button opens it too
    fx.hl.click(&mut fx.app, button.center() - egui::vec2(BUTTON_RADIUS + 20.0, 0.0), PointerButton::Primary);
    fx.hl.frames(&mut fx.app, 3);
    assert_eq!(opened.lock().unwrap().len(), 2);
}

// spec: SYNC-020, MEDIA-010
#[test]
fn a_video_that_is_an_lfs_pointer_is_downloaded_first_even_without_ffmpeg() {
    let (_guard, opened) = pretend_player();
    let mut fx = Fx::new_with("v-pointer", VIDEO_NOTE, &[], |a| a.media.use_ffmpeg(|| None));
    let clip = fx.dir.join("clip.mp4");
    super::write_pointer(&clip, 2_000_000);
    fx.app.vault.as_mut().unwrap().rescan();
    fx.reading(true);
    fx.hl.frames(&mut fx.app, 3);
    // the pointer is not probed: it says "not downloaded", not "ffmpeg is not installed"
    assert_eq!(fx.app.media.notice(&clip).map(|n| n.line()), Some("Not downloaded (2 MB)".to_string()));
    assert!(!texts(&fx).iter().any(|t| t.contains("ffmpeg is not installed")), "{:?}", texts(&fx));
    // Play asks for the download; the system player does not get the pointer file
    let ctx = egui::Context::default();
    fx.app.media.toggle(&ctx, &clip, Path::new("Watch.md"));
    assert_eq!(fx.app.media.take_download_requests(), vec![clip.clone()]);
    assert!(opened.lock().unwrap().is_empty(), "{:?}", opened.lock().unwrap());
    assert_eq!(fx.app.media.current(), None);
}

// spec: MEDIA-010
#[test]
fn the_settings_say_where_ffmpeg_is_or_that_it_is_missing_and_where_to_get_it() {
    let mut fx = Fx::new_with("v-settings", VIDEO_NOTE, &["clip.mp4"], |a| a.media.use_ffmpeg(|| None));
    fx.app.open_settings(SettingsTab::Appearance);
    fx.hl.frames(&mut fx.app, 3);
    assert!(texts(&fx).iter().any(|t| t.contains("ffmpeg is not installed")), "{:?}", texts(&fx));
    assert!(texts(&fx).iter().any(|t| t.contains("ffmpeg.org")), "it says where to get it");
    if have_ffmpeg() {
        let mut fx = Fx::new("v-settings2", VIDEO_NOTE, &["clip.mp4"]);
        fx.app.open_settings(SettingsTab::Appearance);
        fx.hl.frames(&mut fx.app, 3);
        assert!(texts(&fx).iter().any(|t| t.starts_with("Videos are played with ffmpeg")), "{:?}", texts(&fx));
    }
}
