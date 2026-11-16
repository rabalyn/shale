//! The player card driven in the whole window with real pointer and key events (MEDIA-001,
//! MEDIA-003…008). The sound goes into a thread that only keeps time (`use_silent_output`), so
//! the tests need no sound card and make no noise.

use super::testdata::fixture;
use super::widget::BUTTON_RADIUS;
use crate::app::{Options, SettingsTab, ShaleApp};
use crate::ui_test_support::Headless;
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, Rect};
use std::path::{Path, PathBuf};
use std::time::Duration;

mod video;

struct Fx {
    app: ShaleApp,
    hl: Headless,
    dir: PathBuf,
}

impl Fx {
    /// A vault with `note` as `Player.md` and these sound files from the fixtures, the note open.
    fn new(tag: &str, note: &str, sounds: &[&str]) -> Fx {
        Fx::new_with(tag, note, sounds, |_| {})
    }

    /// Like `new`, with `setup` run on the app before the note is opened.
    fn new_with(tag: &str, note: &str, sounds: &[&str], setup: impl FnOnce(&mut ShaleApp)) -> Fx {
        let dir = std::env::temp_dir().join(format!("shale-media-ui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for s in sounds {
            if s.starts_with("bad.") {
                // not a sound at all
                // (ffprobe takes bytes of 9 for a raw video)
                std::fs::write(dir.join(s), vec![if s.ends_with("mp4") { 7u8 } else { 9u8 }; 3000]).unwrap();
            } else {
                std::fs::copy(fixture(s), dir.join(s)).unwrap();
            }
        }
        std::fs::write(dir.join("Player.md"), note).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.media.use_silent_output();
        setup(&mut app);
        app.open(Path::new("Player.md"), false);
        let mut hl = Headless::new();
        hl.frames(&mut app, 4);
        Fx { app, hl, dir }
    }

    fn reading(&mut self, on: bool) {
        let a = self.app.tabbar.active;
        self.app.tabbar.tabs[a].reading = on;
        self.hl.frames(&mut self.app, 3);
    }

    /// Frames until the app is in the state `ok` looks for (sound opens on threads).
    fn until(&mut self, what: &str, mut ok: impl FnMut(&ShaleApp) -> bool) {
        for _ in 0..300 {
            self.hl.frame(&mut self.app, vec![]);
            if ok(&self.app) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("timed out waiting for: {what}");
    }

    fn label(&mut self, label: &str) -> Rect {
        self.hl.find_label(&mut self.app, label)
    }

    fn click_label(&mut self, label: &str) {
        let r = self.label(label);
        self.hl.click(&mut self.app, r.center(), PointerButton::Primary);
    }

    /// Play the first player and wait until its output is open.
    fn play(&mut self) {
        self.click_label("Play");
        self.until("the output is open", |a| a.media.position().is_some());
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn circles(out: &egui::FullOutput) -> Vec<(f32, egui::Color32)> {
    fn walk(s: &egui::Shape, v: &mut Vec<(f32, egui::Color32)>) {
        match s {
            egui::Shape::Circle(c) => v.push((c.radius, c.fill)),
            egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, v)),
            _ => {}
        }
    }
    let mut v = Vec::new();
    out.shapes.iter().for_each(|c| walk(&c.shape, &mut v));
    v
}

const NOTE: &str = "# Listen\n\n![[tone-cover.mp3]]\n\nafter the player\n";

// spec: MEDIA-001, MEDIA-004
#[test]
fn the_reading_view_shows_a_player_and_the_button_plays_and_pauses() {
    let mut fx = Fx::new("toggle", NOTE, &["tone-cover.mp3"]);
    fx.reading(true);
    fx.label("Play");
    fx.label("Mute");
    fx.label("Volume");
    assert_eq!(fx.app.media.current(), None, "nothing starts by itself (MEDIA-007)");
    fx.click_label("Play");
    assert_eq!(fx.app.media.current(), Some(fx.dir.join("tone-cover.mp3")));
    fx.label("Pause");
    fx.until("the sound runs", |a| a.media.position().is_some_and(|p| p > 0.1));
    fx.click_label("Pause");
    fx.label("Play");
    // paused: the position stands still
    fx.hl.frames(&mut fx.app, 3);
    std::thread::sleep(Duration::from_millis(60));
    let p = fx.app.media.position().unwrap();
    std::thread::sleep(Duration::from_millis(150));
    assert!((fx.app.media.position().unwrap() - p).abs() < 0.001, "no progress while paused");
}

// spec: MEDIA-001
#[test]
fn the_player_is_in_the_editor_and_in_the_markdown_preview_as_well() {
    let mut fx = Fx::new("editor", NOTE, &["tone-cover.mp3"]);
    fx.reading(false);
    fx.label("Play");
    fx.app.cfg.live_preview = true;
    fx.hl.frames(&mut fx.app, 4);
    fx.label("Play");
}

// spec: MEDIA-001
#[test]
fn the_extension_decides_in_any_case_and_a_link_without_the_bang_is_no_player() {
    for name in ["a.MP3", "b.Ogg", "c.oga", "d.OPUS", "e.flac", "f.m4a", "g.aac", "h.wav"] {
        assert!(super::is_audio(Path::new(name)), "{name}");
    }
    for name in ["a.mp4", "b.png", "c", "d.mp3.txt", "e.md"] {
        assert!(!super::is_audio(Path::new(name)), "{name}");
    }
    let mut fx = Fx::new("link", "just [[tone.wav]] linked\n", &["tone.wav"]);
    fx.reading(true);
    assert!(!fx.hl.text.iter().any(|(t, _)| t == "Play"));
    let out = fx.hl.frame(&mut fx.app, vec![]);
    assert!(circles(&out).iter().all(|(r, _)| *r != BUTTON_RADIUS), "no round button");
}

// spec: MEDIA-003
#[test]
fn without_a_cover_a_tile_shows_the_file_name() {
    let mut fx = Fx::new("tile", "![[tone.wav]]\n\n![[tone-cover.flac]]\n", &["tone.wav", "tone-cover.flac"]);
    fx.reading(true);
    std::thread::sleep(Duration::from_millis(300));
    fx.hl.frames(&mut fx.app, 3);
    // (the file list in the side panel names the files too: count only the note's column)
    let count = |fx: &Fx, name: &str| fx.hl.text.iter().filter(|(t, p)| t == name && p.x > 300.0).count();
    assert_eq!(count(&fx, "tone.wav"), 2, "the name in the title and on the neutral tile");
    assert_eq!(count(&fx, "tone-cover.flac"), 1, "with a cover only in the title");
}

// spec: MEDIA-004
#[test]
fn the_button_has_the_opacity_of_the_setting_and_is_opaque_under_the_mouse() {
    let mut fx = Fx::new("opacity", NOTE, &["tone-cover.mp3"]);
    fx.reading(true);
    let alpha = |out: &egui::FullOutput| circles(out).iter().find(|(r, _)| *r == BUTTON_RADIUS).map(|(_, c)| c.a()).expect("the round button");
    let out = fx.hl.frame(&mut fx.app, vec![]);
    assert_eq!(alpha(&out), 153, "0.6 by default");
    fx.app.cfg.media_play_button_opacity = 0.3;
    fx.hl.frames(&mut fx.app, 2);
    let out = fx.hl.frame(&mut fx.app, vec![]);
    assert_eq!(alpha(&out), 77);
    let c = fx.label("Play").center();
    fx.hl.frame(&mut fx.app, vec![Event::PointerMoved(c)]);
    let out = fx.hl.frame(&mut fx.app, vec![Event::PointerMoved(c)]);
    assert_eq!(alpha(&out), 255, "fully opaque on hover");
    fx.hl.frame(&mut fx.app, vec![Event::PointerMoved(Pos2::new(900.0, 10.0))]);
    let out = fx.hl.frame(&mut fx.app, vec![]);
    assert_eq!(alpha(&out), 77, "and back");
}

// spec: MEDIA-004
#[test]
fn space_toggles_while_the_player_has_the_keyboard() {
    let mut fx = Fx::new("space", NOTE, &["tone-cover.mp3"]);
    fx.reading(true);
    let c = fx.label("Play").center();
    // a click on the card beside the button gives it the keyboard
    let corner = c - egui::vec2(super::widget::BUTTON_RADIUS * 2.0 + 4.0, 40.0);
    let space = |pressed| Event::Key { key: Key::Space, physical_key: Some(Key::Space), pressed, repeat: false, modifiers: Modifiers::NONE };
    fx.hl.click(&mut fx.app, corner, PointerButton::Primary);
    assert_eq!(fx.app.media.current(), None, "a click beside the button does not play");
    fx.hl.frame(&mut fx.app, vec![space(true)]);
    fx.hl.frame(&mut fx.app, vec![space(false)]);
    fx.hl.frames(&mut fx.app, 2);
    assert!(fx.app.media.current().is_some(), "Space starts it");
    fx.label("Pause");
    fx.hl.frame(&mut fx.app, vec![space(true)]);
    fx.hl.frame(&mut fx.app, vec![space(false)]);
    fx.hl.frames(&mut fx.app, 2);
    fx.label("Play");
}

// spec: MEDIA-005
#[test]
fn a_click_on_the_progress_bar_jumps_and_a_drag_seeks_once_on_release() {
    let mut fx = Fx::new("seek", NOTE, &["tone-cover.mp3"]);
    fx.reading(true);
    fx.play();
    fx.click_label("Pause");
    let bar = fx.label("Position");
    let at = |f: f32| Pos2::new(bar.left() + bar.width() * f, bar.center().y);
    // click
    fx.hl.click(&mut fx.app, at(0.5), PointerButton::Primary);
    let p = fx.app.media.position().unwrap();
    assert!((0.4..0.65).contains(&p), "a click in the middle jumps to the middle of the one second: {p}");
    // drag: nothing is sought while the mouse is down
    let before = fx.app.media.position().unwrap();
    let path = Headless::steps(at(0.2), at(0.9), 6);
    fx.hl.press_and_move(&mut fx.app, at(0.2), &path);
    assert_eq!(fx.app.media.position().unwrap(), before, "dragging only previews");
    let shown = fx.hl.text.iter().any(|(t, _)| t.starts_with("0:0"));
    assert!(shown, "the time shows the target");
    fx.hl.release(&mut fx.app, at(0.9));
    let p = fx.app.media.position().unwrap();
    assert!((0.8..1.05).contains(&p), "the release seeks: {p}");
}

// spec: MEDIA-006
#[test]
fn mute_and_the_volume_slider_change_the_one_global_volume() {
    let mut fx = Fx::new("volume", NOTE, &["tone-cover.mp3"]);
    fx.reading(true);
    let v = fx.label("Volume");
    fx.hl.click(&mut fx.app, Pos2::new(v.left() + v.width() * 0.25, v.center().y), PointerButton::Primary);
    assert!((fx.app.cfg.media_volume - 0.25).abs() < 0.05, "the slider sets the config volume: {}", fx.app.cfg.media_volume);
    fx.click_label("Mute");
    fx.label("Unmute");
    assert!((fx.app.cfg.media_volume - 0.25).abs() < 0.05, "muting does not forget the volume");
    fx.click_label("Unmute");
    fx.label("Mute");
}

// spec: MEDIA-007
#[test]
fn closing_the_tab_stops_the_sound_and_so_does_switching_the_vault() {
    let mut fx = Fx::new("stop", NOTE, &["tone-cover.mp3"]);
    fx.reading(true);
    fx.play();
    let a = fx.app.tabbar.active;
    fx.app.close_tab(a);
    assert_eq!(fx.app.media.current(), None, "the tab was closed");

    let mut fx = Fx::new("stop2", NOTE, &["tone-cover.mp3"]);
    fx.reading(true);
    fx.play();
    let other = std::env::temp_dir().join(format!("shale-media-ui-other-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&other);
    crate::vault::write_sample_vault(&other).unwrap();
    fx.app.open_vault(&other);
    assert_eq!(fx.app.media.current(), None, "another vault was opened");
    let _ = std::fs::remove_dir_all(&other);
}

// spec: MEDIA-008
#[test]
fn the_state_stays_when_the_view_switches_and_the_window_repaints_only_while_playing() {
    let mut fx = Fx::new("state", NOTE, &["tone-cover.mp3"]);
    fx.reading(true);
    // idle: no repaint is asked for (the cover has been read by now)
    std::thread::sleep(Duration::from_millis(300));
    fx.hl.frames(&mut fx.app, 3);
    let delay = |out: &egui::FullOutput| out.viewport_output.values().map(|v| v.repaint_delay).min().unwrap();
    let out = fx.hl.frame(&mut fx.app, vec![]);
    assert!(delay(&out) > Duration::from_secs(1), "idle: {:?}", delay(&out));
    fx.play();
    let out = fx.hl.frame(&mut fx.app, vec![]);
    assert!(delay(&out) <= Duration::from_millis(100), "playing: {:?}", delay(&out));
    fx.until("some time passed", |a| a.media.position().is_some_and(|p| p > 0.15));
    // editor, preview and reading view show the same state
    for (reading, preview) in [(false, false), (false, true), (true, true)] {
        fx.app.cfg.live_preview = preview;
        fx.reading(reading);
        fx.label("Pause");
        assert!(fx.app.media.position().unwrap() > 0.1, "the position goes on in every view");
    }
    fx.click_label("Pause");
    // away from the button (a hover animation asks for repaints too)
    fx.hl.frame(&mut fx.app, vec![Event::PointerMoved(Pos2::new(900.0, 10.0))]);
    fx.hl.frames(&mut fx.app, 8);
    std::thread::sleep(Duration::from_millis(200));
    let out = fx.hl.frame(&mut fx.app, vec![]);
    assert!(delay(&out) > Duration::from_secs(1), "paused again: {:?}", delay(&out));
}

// spec: MEDIA-002
#[test]
fn a_damaged_file_says_so_and_a_machine_without_sound_output_keeps_the_player_working() {
    let mut fx = Fx::new("damaged", "![[bad.mp3]]\n", &["bad.mp3"]);
    fx.reading(true);
    std::thread::sleep(Duration::from_millis(300));
    fx.hl.frames(&mut fx.app, 3);
    assert!(fx.hl.text.iter().any(|(t, _)| t.contains("damaged") || t.contains("can't")), "the player tells why: {:?}", fx.hl.text.iter().map(|(t, _)| t.clone()).collect::<Vec<_>>());

    // no sound output: the message shows, nothing panics, the card still works
    let mut fx = Fx::new("nodevice", NOTE, &["tone-cover.mp3"]);
    fx.app.media.use_no_output();
    fx.reading(true);
    fx.click_label("Play");
    fx.until("the message shows", |a| a.media.current().is_some());
    fx.hl.frames(&mut fx.app, 5);
    std::thread::sleep(Duration::from_millis(100));
    fx.hl.frames(&mut fx.app, 3);
    assert!(fx.hl.text.iter().any(|(t, _)| t == "No sound output"));
    fx.label("Position");
    fx.label("Mute");
}

// spec: MEDIA-004, MEDIA-007
#[test]
fn the_settings_dialog_has_the_button_opacity_and_nothing_to_autoplay() {
    let mut fx = Fx::new("settings", NOTE, &["tone-cover.mp3"]);
    fx.app.open_settings(SettingsTab::General);
    fx.hl.frames(&mut fx.app, 3);
    let json = serde_json::to_string(&fx.app.cfg).unwrap();
    assert!(json.contains("\"media_play_button_opacity\":0.6"), "{json}");
    assert!(!fx.hl.text.iter().any(|(t, _)| t.to_lowercase().contains("autoplay") || t.contains("when the note is opened")), "nothing starts by itself, so there is no such setting");
    assert!(fx.hl.text.iter().any(|(t, _)| t.contains("Play button over the artwork")), "the opacity slider is in Appearance");
}

fn pointer_text(size: u64) -> String {
    format!("version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {size}\n", "b".repeat(64))
}

// spec: SYNC-020
#[test]
fn a_sound_that_is_an_lfs_pointer_says_not_downloaded_and_the_download_link_asks_for_it() {
    let mut fx = Fx::new("pointer", "![[Song.mp3]]\n", &[]);
    std::fs::write(fx.dir.join("Song.mp3"), pointer_text(12_345_678)).unwrap();
    fx.app.vault.as_mut().unwrap().rescan();
    fx.reading(true);
    let notice = fx.app.media.notice(&fx.dir.join("Song.mp3")).expect("a pointer file");
    assert_eq!(notice.line(), "Not downloaded (12 MB)");
    fx.label("Download");
    assert_eq!(fx.app.media.current(), None, "nothing was decoded or played");
    // the vault is no repository here, so the download says so in words, in the player
    fx.click_label("Download");
    let song = fx.dir.join("Song.mp3");
    fx.until("the download failed", |a| a.media.notice(&song).is_some_and(|n| n.error.is_some()));
    let n = fx.app.media.notice(&song).unwrap();
    assert!(!n.running && n.line().starts_with("Download failed: ") && n.line().contains("not a git repository"), "{}", n.line());
    // "Download" offers it again, and playing does not decode the pointer
    fx.label("Download");
    fx.click_label("Play");
    assert_eq!(fx.app.media.current(), None);
}

// spec: SYNC-020
#[test]
fn pressing_play_on_a_pointer_downloads_first_and_plays_when_the_file_is_there() {
    let mut fx = Fx::new("pointer-play", "![[Song.mp3]]\n", &[]);
    let song = fx.dir.join("Song.mp3");
    std::fs::write(&song, pointer_text(1_000)).unwrap();
    fx.app.vault.as_mut().unwrap().rescan();
    fx.reading(true);
    let ctx = egui::Context::default();
    fx.app.media.toggle(&ctx, &song, Path::new("Player.md"));
    assert_eq!(fx.app.media.current(), None, "asked, nothing plays yet");
    assert!(fx.app.media.notice(&song).unwrap().running);
    assert_eq!(fx.app.media.take_download_requests(), vec![song.clone()]);
    // the download brought the real file
    std::fs::copy(fixture("tone-cover.mp3"), &song).unwrap();
    fx.app.media.download_done(&ctx, &song, Ok(()));
    assert_eq!(fx.app.media.current(), Some(song.clone()));
    fx.until("the sound runs", |a| a.media.position().is_some());
    assert_eq!(fx.app.media.notice(&song), None);
}

// spec: SYNC-020
#[test]
fn a_picture_that_is_an_lfs_pointer_says_not_downloaded_in_the_reading_view_and_the_editor() {
    let mut fx = Fx::new("pointer-picture", "Look:\n\n![[Photo.png]]\n\nafter\n", &[]);
    std::fs::write(fx.dir.join("Photo.png"), pointer_text(2_500_000)).unwrap();
    fx.app.vault.as_mut().unwrap().rescan();
    let photo = fx.dir.join("Photo.png");
    fx.reading(true);
    fx.label("Download");
    assert_eq!(fx.app.media.notice(&photo).unwrap().line(), "Not downloaded (2 MB)");
    fx.reading(false);
    fx.click_label("Download");
    fx.until("asked and failed in words (the vault is no repository)", |a| a.media.notice(&photo).is_some_and(|n| n.error.is_some()));
    // a real file is no business of this
    std::fs::write(fx.dir.join("Other.png"), vec![1u8; 3000]).unwrap();
    assert_eq!(fx.app.media.notice(&fx.dir.join("Other.png")), None);
}
