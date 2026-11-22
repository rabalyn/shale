//! Media outside the vault shown in the whole window (MEDIA-014…MEDIA-018): library links and
//! file links to pictures, sounds and videos in a folder next to the vault, in the reading view, the
//! editor and the markdown preview; what stands there when the file or the library is missing; and
//! what a click does.

#![cfg(test)]

use super::{fixture, Fx};
use crate::media::outside::Library;
use crate::media::testdata::file_address;
use eframe::egui::{self, epaint::Shape, PointerButton, Pos2};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// A folder outside the vault, removed when the test ends. Its subfolder has a space in its name,
/// so that the `%20` of a file address is part of every test.
struct Outside {
    root: PathBuf,
    /// `<root>/My Media`
    folder: PathBuf,
}

impl Outside {
    fn new(tag: &str) -> Outside {
        let root = std::env::temp_dir().join(format!("shale-media-outside-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let folder = root.join("My Media");
        std::fs::create_dir_all(folder.join("2024")).unwrap();
        Outside { root, folder }
    }

    fn picture(&self, name: &str) -> &Self {
        image::RgbaImage::from_pixel(80, 40, image::Rgba([200, 30, 30, 255])).save(self.folder.join(name)).unwrap();
        self
    }

    fn fixture(&self, name: &str, as_name: &str) -> &Self {
        std::fs::copy(fixture(name), self.folder.join(as_name)).unwrap();
        self
    }

    /// The library `Media` of this folder, as this device has it.
    fn library(&self) -> Vec<Library> {
        vec![Library { name: "Media".into(), path: self.folder.clone() }]
    }

    fn address(&self, name: &str) -> String {
        file_address(&self.folder.join(name))
    }
}

impl Drop for Outside {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The window with `note` open; `libraries` are this device's library folders.
fn window(tag: &str, note: &str, libraries: Vec<Library>) -> Fx {
    Fx::new_with(tag, note, &[], move |a| a.cfg.media_libraries = libraries)
}

/// How many pictures a frame draws (textured meshes and rectangles; the font atlas is the default texture).
fn pictures(out: &egui::FullOutput) -> usize {
    fn walk(s: &Shape, n: &mut usize) {
        match s {
            Shape::Mesh(m) if m.texture_id != egui::TextureId::default() => *n += 1,
            Shape::Rect(r) if r.brush.is_some() => *n += 1,
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, n)),
            _ => {}
        }
    }
    let mut n = 0;
    out.shapes.iter().for_each(|c| walk(&c.shape, &mut n));
    n
}

fn drawn(fx: &mut Fx) -> usize {
    fx.hl.frames(&mut fx.app, 3);
    let out = fx.hl.frame(&mut fx.app, vec![]);
    pictures(&out)
}

fn said(fx: &Fx, what: &str) -> bool {
    fx.hl.text.iter().any(|(t, _)| t.contains(what))
}

/// The three ways to look at a note: the reading view, the editor and the markdown preview.
fn views(fx: &mut Fx, mut check: impl FnMut(&mut Fx, &str)) {
    fx.reading(true);
    check(fx, "the reading view");
    fx.reading(false);
    check(fx, "the editor");
    fx.app.cfg.live_preview = true;
    fx.hl.frames(&mut fx.app, 4);
    check(fx, "the markdown preview");
}

/// A pretend system program: gets what would be opened.
fn pretend_program() -> (crate::opener::BrowserGuard, Arc<Mutex<Vec<String>>>) {
    let opened = Arc::new(Mutex::new(Vec::new()));
    let sink = opened.clone();
    (crate::opener::install_test_browser(Box::new(move |t| sink.lock().unwrap().push(t.to_string()))), opened)
}

// spec: MEDIA-014, MEDIA-016
#[test]
fn a_picture_of_a_library_shows_in_the_reading_view_the_editor_and_the_preview() {
    let o = Outside::new("lib-picture");
    o.picture("2024/a.png");
    let mut fx = window("o-lib-pic", "# Pictures\n\n![[@Media/2024/a.png]]\n\nafter\n", o.library());
    views(&mut fx, |fx, view| assert_eq!(drawn(fx), 1, "{view} shows the picture"));
    // this device has no such library: the same note says so instead
    fx.app.cfg.media_libraries.clear();
    fx.reading(true);
    assert_eq!(drawn(&mut fx), 0);
    assert!(said(&fx, "Library “Media” is not set up on this device"), "{:?}", fx.hl.text);
}

// spec: MEDIA-015, MEDIA-016
#[test]
fn a_picture_by_file_address_shows_with_a_space_in_its_folder_as_written_or_as_percent_20() {
    let o = Outside::new("file-picture");
    o.picture("b.png");
    let encoded = o.address("b.png");
    // a file address with %20, and one with the space written inside angle brackets (Obsidian's form)
    assert!(encoded.contains("My%20Media"));
    for (n, link) in [format!("![]({encoded})"), format!("![](<{}>)", encoded.replace("%20", " "))].iter().enumerate() {
        let mut fx = window(&format!("o-file-pic{n}"), &format!("{link}\n"), vec![]);
        views(&mut fx, |fx, view| assert_eq!(drawn(fx), 1, "{view} shows the picture of {link}"));
    }
}

// spec: MEDIA-014, MEDIA-015, MEDIA-001
#[test]
fn a_sound_of_a_library_and_one_by_file_address_get_a_player_that_plays() {
    let o = Outside::new("sound");
    o.fixture("tone.wav", "2024/tone.wav");
    for (n, link) in ["![[@Media/2024/tone.wav]]".to_string(), format!("![]({})", o.address("2024/tone.wav"))].into_iter().enumerate() {
        let mut fx = window(&format!("o-sound{n}"), &format!("# Listen\n\n{link}\n"), o.library());
        fx.app.media.use_silent_output();
        views(&mut fx, |fx, view| {
            fx.label("Play");
            assert!(!fx.hl.text.iter().any(|(t, _)| t.contains("Not found")), "{view}");
        });
        fx.reading(true);
        fx.click_label("Play");
        assert_eq!(fx.app.media.current(), Some(o.folder.join("2024/tone.wav")), "{link}: it plays the file where it is");
        fx.label("Pause");
        fx.click_label("Pause");
    }
}

// spec: MEDIA-014, MEDIA-017, MEDIA-010
#[test]
fn a_video_outside_the_vault_without_ffmpeg_opens_in_the_system_player() {
    let (_guard, opened) = pretend_program();
    let o = Outside::new("video");
    o.fixture("clip.mp4", "clip.mp4");
    let mut fx = Fx::new_with("o-video", "![[@Media/clip.mp4]]\n", &[], |a| a.media.use_ffmpeg(|| None));
    fx.app.cfg.media_libraries = o.library();
    fx.reading(true);
    let shown = fx.until_text("ffmpeg is not installed");
    assert!(shown.iter().any(|t| t.contains("ffmpeg is not installed")), "{shown:?}");
    fx.click_label("Play");
    fx.hl.frames(&mut fx.app, 3);
    assert_eq!(fx.app.media.current(), None, "nothing plays inside Shale");
    let o = opened.lock().unwrap();
    assert_eq!(o.len(), 1, "{o:?}");
    assert!(o[0].ends_with("clip.mp4") && o[0].contains("My Media"), "the file where it is: {o:?}");
}

// spec: MEDIA-016
#[test]
fn a_file_that_is_not_there_a_library_not_set_up_and_a_path_out_of_a_library_say_so() {
    let o = Outside::new("states");
    std::fs::write(o.root.join("secret.png"), b"x").unwrap();
    let gone = o.folder.join("gone.png");
    let note = format!("![[@Media/gone.png]]\n\n![[@Nowhere/a.mp4]]\n\n![[@Media/../secret.png]]\n\n![](<{}>)\n", o.address("2024/b.mp4"));
    let mut fx = window("o-states", &note, o.library());
    let before = fx.app.vault.as_ref().unwrap().files.len();
    views(&mut fx, |fx, view| {
        fx.hl.frames(&mut fx.app, 3);
        assert!(said(fx, &format!("Not found: {}", gone.display())), "{view}: the path of the picture: {:?}", fx.hl.text);
        assert!(said(fx, "Library “Nowhere” is not set up on this device"), "{view}");
        assert!(said(fx, "leaves the library folder “Media”"), "{view}");
        assert!(said(fx, &format!("Not found: {}", o.folder.join("2024/b.mp4").display())), "{view}");
        assert_eq!(drawn(fx), 0, "{view}: nothing to draw");
    });
    assert_eq!(fx.app.vault.as_ref().unwrap().files.len(), before, "no note was made");
}

// spec: MEDIA-017
#[test]
fn a_click_on_an_outside_link_opens_the_file_with_the_system_program_and_never_makes_a_note() {
    let (_guard, opened) = pretend_program();
    let o = Outside::new("click");
    o.fixture("tone.wav", "tone.wav");
    let note = format!("[[@Media/tone.wav]]\n\n[[@Media/gone.mp3]]\n\n[[@Nowhere/b.mp3]]\n\n[label](<{}>)\n", o.address("tone.wav"));
    let mut fx = window("o-click", &note, o.library());
    fx.reading(true);
    let before = fx.app.vault.as_ref().unwrap().files.len();
    let click_on = |fx: &mut Fx, text: &str| {
        fx.hl.frames(&mut fx.app, 2);
        let at = fx.hl.text.iter().find(|(t, _)| t.contains(text)).map(|(_, p)| *p).unwrap_or_else(|| panic!("{text:?} is not on the screen: {:?}", fx.hl.text));
        fx.hl.click(&mut fx.app, at + egui::vec2(6.0, 6.0), PointerButton::Primary);
    };
    click_on(&mut fx, "@Media/tone.wav");
    assert_eq!(opened.lock().unwrap().len(), 1);
    assert!(opened.lock().unwrap()[0].ends_with("tone.wav"));
    click_on(&mut fx, "label");
    assert_eq!(opened.lock().unwrap().len(), 2, "the file address too");
    // nothing there: a message, nothing opened, no note
    click_on(&mut fx, "@Media/gone.mp3");
    assert!(fx.app.toasts.iter().any(|(t, _)| t.starts_with("Not found:") && t.ends_with("gone.mp3")), "{:?}", fx.app.toasts);
    click_on(&mut fx, "@Nowhere/b.mp3");
    assert!(fx.app.toasts.iter().any(|(t, _)| t == "Library “Nowhere” is not set up on this device"), "{:?}", fx.app.toasts);
    assert_eq!(opened.lock().unwrap().len(), 2);
    assert_eq!(fx.app.vault.as_ref().unwrap().files.len(), before, "no note was made");
    assert!(!fx.dir.join("gone.mp3.md").exists() && !fx.dir.join("b.mp3.md").exists());
}

// spec: MEDIA-017
#[test]
fn a_click_on_the_caption_of_an_outside_picture_in_the_editor_opens_it_too() {
    let (_guard, opened) = pretend_program();
    let o = Outside::new("caption");
    o.picture("a.png");
    let mut fx = window("o-caption", "![[@Media/a.png]]\n", o.library());
    fx.reading(false);
    fx.hl.frames(&mut fx.app, 4);
    let at = fx.hl.text.iter().find(|(t, _)| t == "@Media/a.png").map(|(_, p)| *p).expect("the caption of the picture");
    fx.hl.click(&mut fx.app, Pos2::new(at.x + 6.0, at.y + 5.0), PointerButton::Primary);
    assert!(opened.lock().unwrap().iter().any(|p| p.ends_with("a.png")), "{:?} {:?}", opened.lock().unwrap(), fx.app.toasts);
}

// spec: MEDIA-018
#[test]
fn the_outgoing_links_panel_lists_outside_media_apart_and_not_as_missing_notes() {
    let o = Outside::new("panel");
    o.picture("a.png");
    let note = "[[@Media/a.png]] and [[@Nowhere/b.mp3]] and [[Missing note]]\n";
    let mut fx = window("o-panel", note, o.library());
    fx.app.side.layout.show_panel(crate::app::RightPanel::Outgoing);
    fx.hl.frames(&mut fx.app, 4);
    assert!(said(&fx, "Outside the vault  2"), "{:?}", fx.hl.text);
    assert!(said(&fx, "Not created yet  1"), "only the missing note is one: {:?}", fx.hl.text);
    assert!(said(&fx, "@Media/a.png") && said(&fx, "@Nowhere/b.mp3"));
}

// spec: MEDIA-014, MEDIA-016, VAULT-018
#[test]
fn showing_media_from_outside_changes_nothing_there_and_adds_nothing_to_the_vault() {
    let o = Outside::new("readonly");
    o.picture("a.png");
    let before: Vec<_> = std::fs::read_dir(&o.folder).unwrap().flatten().map(|e| (e.file_name(), e.metadata().unwrap().len())).collect();
    let mut fx = window("o-readonly", "![[@Media/a.png]] ![[@Media/../x.png]]\n\n![](<file:///nowhere/x.png>)\n", o.library());
    views(&mut fx, |fx, _| fx.hl.frames(&mut fx.app, 3));
    let after: Vec<_> = std::fs::read_dir(&o.folder).unwrap().flatten().map(|e| (e.file_name(), e.metadata().unwrap().len())).collect();
    assert_eq!(before.len(), after.len());
    assert!(fx.app.vault.as_ref().unwrap().files.iter().all(|f| !f.rel.starts_with("..") && !f.rel.is_absolute()), "the vault's files are the vault's");
}
