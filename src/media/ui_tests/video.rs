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
        // the length is known, so the click on the bar below jumps at once
        fx.until("the look is done", |a| a.media.thumbnail_size(&abs).is_some());
        // the clock starts with the first picture, however slow ffmpeg is (#259)
        fx.click_label("Play");
        fx.until("the output is open", |a| a.media.position().is_some());
        fx.until("a frame of the video is shown", |a| a.media.picture_size().is_some());
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

/// Holds ffprobe back until the test lets it go, so the length of a video is not known yet.
static LENGTH_HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn held_ffmpeg() -> Option<crate::media::ffmpeg::Tools> {
    while LENGTH_HELD.load(std::sync::atomic::Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(10));
    }
    crate::media::ffmpeg::locate()
}

// spec: MEDIA-005
#[test]
fn a_click_on_the_bar_before_the_length_is_known_waits_for_it_and_then_jumps() {
    if !have_ffmpeg() {
        return;
    }
    let mut fx = Fx::new_with("v-early-seek", VIDEO_NOTE, &["clip.mp4"], |a| a.media.use_ffmpeg(held_ffmpeg));
    fx.reading(true);
    let bar = fx.label("Position");
    assert!(texts(&fx).iter().any(|t| t == "–:––"), "the length is not known yet: {:?}", texts(&fx));
    fx.hl.click(&mut fx.app, Pos2::new(bar.left() + bar.width() * 0.8, bar.center().y), PointerButton::Primary);
    fx.hl.frames(&mut fx.app, 3);
    let waiting = fx.app.media.waiting_share().expect("the click is remembered (#258)");
    assert!((0.75..0.85).contains(&waiting), "{waiting}");
    assert!(fx.app.media.position().is_none_or(|p| p == 0.0), "nothing is sought while the length is unknown");
    LENGTH_HELD.store(false, std::sync::atomic::Ordering::Relaxed);
    fx.until("the length arrives and the click jumps", |a| a.media.waiting_share().is_none() && !a.media.seek_pending() && a.media.position().is_some_and(|p| p > 0.6));
    assert!(texts(&fx).iter().any(|t| t == "0:01"), "the length shows: {:?}", texts(&fx));
    fx.label("Play");
}

// spec: MEDIA-013
#[test]
fn a_video_on_a_machine_without_sound_output_plays_its_pictures_with_a_note() {
    if !have_ffmpeg() {
        return;
    }
    for clip in ["clip.mp4", "clip-silent.mp4"] {
        let mut fx = Fx::new("v-nodevice", &format!("![[{clip}]]\n"), &[clip]);
        fx.app.media.use_no_output();
        fx.reading(true);
        let abs = fx.dir.join(clip);
        fx.until("the look is done", |a| a.media.thumbnail_size(&abs).is_some());
        fx.click_label("Play");
        fx.until("a frame of the video is shown", |a| a.media.picture_size().is_some());
        fx.until("the video runs", |a| a.media.position().is_some_and(|p| p > 0.15));
        fx.hl.frames(&mut fx.app, 3);
        assert!(texts(&fx).iter().any(|t| t == "No sound output: playing without sound"), "{clip}: the note shows: {:?}", texts(&fx));
        fx.label("Pause");
        fx.label("Position");
        fx.click_label("Pause");
        fx.hl.frames(&mut fx.app, 3);
        std::thread::sleep(Duration::from_millis(80));
        let p = fx.app.media.position().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!((fx.app.media.position().unwrap() - p).abs() < 0.001, "{clip}: paused");
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
    // the probe by ffmpeg is slower on a busy machine: wait for the words, not for a time
    let shown = fx.until_text("damaged");
    assert!(shown.iter().any(|t| t.contains("damaged")), "{shown:?}");
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
    let shown = fx.until_text("ffmpeg is not installed");
    assert!(shown.iter().any(|t| t.contains("ffmpeg is not installed")), "{shown:?}");
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
    fx.disk_changed();
    fx.reading(true);
    fx.hl.frames(&mut fx.app, 3);
    // the pointer is not probed: it says "not downloaded", not "ffmpeg is not installed"
    assert_eq!(fx.app.media.notice(fx.hl.ctx(), &clip).map(|n| n.line()), Some("Not downloaded (2 MB)".to_string()));
    assert!(!texts(&fx).iter().any(|t| t.contains("ffmpeg is not installed")), "{:?}", texts(&fx));
    // Play asks for the download; the system player does not get the pointer file
    let ctx = fx.hl.ctx().clone();
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

// spec: MEDIA-004
#[test]
fn in_the_editor_the_round_buttons_the_edge_and_the_picture_of_a_video_leave_the_keyboard_with_the_text() {
    use eframe::egui::{
        text::{CCursor, CCursorRange},
        text_edit::TextEditState,
        Event,
    };
    if !have_ffmpeg() {
        return;
    }
    let mut fx = Fx::new("v-editor-keys", VIDEO_NOTE, &["clip.mp4"]);
    fx.reading(false);
    fx.app.cfg.live_preview = true;
    fx.hl.frames(&mut fx.app, 3);
    let abs = fx.dir.join("clip.mp4");
    fx.until("the look is done", |a| a.media.thumbnail_size(&abs).is_some());
    let a = fx.app.tabbar.active;
    let id = fx.app.tabbar.tabs[a].editor.id;
    fx.app.focus_editor();
    fx.hl.frames(&mut fx.app, 3);
    // the cursor in "after the video", between "af" and "ter"
    let mut st = TextEditState::load(fx.hl.ctx(), id).unwrap_or_default();
    st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(26))));
    st.store(fx.hl.ctx(), id);
    fx.hl.frames(&mut fx.app, 3);
    let note = |fx: &Fx| fx.app.notes[Path::new("Player.md")].text.clone();
    let typed = std::cell::RefCell::new(String::new());
    let check = |fx: &mut Fx, what: &str, c: &str| {
        fx.hl.frames(&mut fx.app, 3);
        assert!(fx.hl.ctx().memory(|m| m.has_focus(id)), "{what}: the text keeps the keyboard");
        fx.hl.frame(&mut fx.app, vec![Event::Text(c.into())]);
        fx.hl.frames(&mut fx.app, 2);
        // the letter lands where the cursor was, behind the link, though a width written into the link moved the text (EDIT-052)
        typed.borrow_mut().push_str(c);
        assert!(note(fx).ends_with(&format!("\n\naf{}ter the video\n", typed.borrow())), "{what}: {:?}", note(fx));
    };

    // the picture of the video, beside the round play button
    let play = fx.label("Play").center();
    fx.hl.click(&mut fx.app, play + egui::vec2(40.0, 30.0), PointerButton::Primary);
    check(&mut fx, "a click on the picture", "A");

    // the round buttons appear under the mouse
    fx.hl.frame(&mut fx.app, vec![Event::PointerMoved(play + egui::vec2(40.0, 30.0))]);
    fx.hl.frames(&mut fx.app, 2);
    fx.click_label("100 %");
    check(&mut fx, "the 100 % button", "B");
    let play = fx.label("Play").center();
    fx.hl.frame(&mut fx.app, vec![Event::PointerMoved(play + egui::vec2(40.0, 30.0))]);
    fx.hl.frames(&mut fx.app, 2);
    fx.click_label("Fullscreen");
    fx.hl.frames(&mut fx.app, 2);
    fx.click_label("Close");
    check(&mut fx, "the fullscreen view, closed again", "C");

    // a drag on the right edge
    let play = fx.label("Play").center();
    let edge = Pos2::new(fx.label("Position").right() + 12.0, play.y);
    fx.hl.press_and_move(&mut fx.app, edge, &crate::ui_test_support::Headless::steps(edge, edge - egui::vec2(30.0, 0.0), 4));
    fx.hl.release(&mut fx.app, edge - egui::vec2(30.0, 0.0));
    fx.hl.frames(&mut fx.app, 2);
    assert!(fx.app.notes[Path::new("Player.md")].text.contains("clip.mp4|"), "the drag wrote the width: {:?}", note(&fx));
    fx.hl.frames(&mut fx.app, 2);
    check(&mut fx, "the drag on the edge", "D");
}
