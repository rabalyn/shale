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

fn note_text(fx: &Fx) -> String {
    fx.app.notes.get(std::path::Path::new("Player.md")).expect("the note is open").text.clone()
}

/// Frames until the copy into the vault has ended and the note has been changed by it.
fn copied(fx: &mut Fx) {
    fx.until("the copy into the vault ends", |a| a.outside_copy.is_none());
    fx.hl.frames(&mut fx.app, 4);
}

fn toast_with(fx: &Fx, what: &str) -> bool {
    fx.app.toasts.iter().any(|(t, _)| t.contains(what))
}

/// Where the link of a note points after "Copy into the vault": the path in `z_media` and its bytes.
fn copy_of(fx: &Fx, link: &str, name: &str) -> PathBuf {
    let rel = link.trim_start_matches("![[").split(['|', ']']).next().unwrap().to_string();
    assert!(rel.starts_with("z_media/") && rel.ends_with(name), "{link}");
    fx.dir.join(rel)
}

// spec: MEDIA-021, EDIT-112
#[test]
fn copy_into_the_vault_on_the_caption_copies_the_file_keeps_the_width_and_is_one_undo_step() {
    let o = Outside::new("copy-caption");
    o.picture("2024/a.png");
    let note = "intro\n\n![[@Media/2024/a.png|300]]\n\nafter the picture\n";
    let mut fx = window("o-copy-caption", note, o.library());
    fx.reading(false);
    let id = fx.app.tabbar.tabs[fx.app.tabbar.active].editor.id;
    // the cursor stands in "after"; a click on the button does not move it
    fx.app.focus_editor();
    fx.hl.frames(&mut fx.app, 3);
    let cursor = note.find("after").unwrap() + 2;
    let mut st = egui::text_edit::TextEditState::load(fx.hl.ctx(), id).unwrap_or_default();
    st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(cursor))));
    st.store(fx.hl.ctx(), id);
    fx.hl.frames(&mut fx.app, 3);
    fx.click_label(crate::media::outside::COPY_LABEL);
    copied(&mut fx);
    let text = note_text(&fx);
    let link = text.lines().nth(2).unwrap();
    assert!(link.ends_with("|300]]"), "the width stays: {link}");
    let copy = copy_of(&fx, link, "_a.png");
    assert_eq!(std::fs::read(&copy).unwrap(), std::fs::read(o.folder.join("2024/a.png")).unwrap(), "a copy of the file");
    assert!(o.folder.join("2024/a.png").is_file(), "the original stays where it is");
    assert!(toast_with(&fx, "Copied 1 file into the vault"), "{:?}", fx.app.toasts);
    // the picture is shown from the vault now, and its caption has no action any more
    assert!(!said(&fx, crate::media::outside::COPY_LABEL) && drawn(&mut fx) == 1);
    // the cursor stands where it stood in the text, and the note is saved by the usual save
    let at = egui::text_edit::TextEditState::load(fx.hl.ctx(), id).and_then(|st| st.cursor.char_range()).map(|r| r.primary.index.0).unwrap();
    assert!(text.chars().skip(at).collect::<String>().starts_with("ter the picture"), "the cursor moved with the text: {at}");
    fx.app.flush_all();
    assert_eq!(std::fs::read_to_string(fx.dir.join("Player.md")).unwrap(), text);
    assert!(fx.app.vault.as_ref().unwrap().files.iter().any(|f| f.rel == copy.strip_prefix(&fx.dir).unwrap()), "the vault knows the new file");
    // one Ctrl+Z brings the outside link back
    fx.hl.frame(&mut fx.app, vec![egui::Event::Key { key: egui::Key::Z, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }]);
    fx.hl.frames(&mut fx.app, 3);
    assert_eq!(note_text(&fx), note, "the undo is one step");
}

// spec: MEDIA-021
#[test]
fn copy_into_the_vault_in_the_reading_view_works_for_a_picture_and_for_a_player_and_saves_the_note() {
    let o = Outside::new("copy-reading");
    o.picture("a.png").fixture("tone.wav", "tone.wav");
    let note = "![[@Media/a.png|120]]\n\n![[@Media/tone.wav]]\n";
    let mut fx = window("o-copy-reading", note, o.library());
    fx.reading(true);
    assert!(said(&fx, crate::media::outside::NOT_SYNCED), "the player's line says it: {:?}", fx.hl.text);
    // the picture's frame has the button next to Open, the player's line has its own
    let buttons = fx.hl.text.iter().filter(|(t, _)| t == crate::media::outside::COPY_LABEL).count();
    assert_eq!(buttons, 2, "{:?}", fx.hl.text);
    // the player's button first (the lower one), then the picture's
    let lower = fx.hl.text.iter().filter(|(t, _)| t == crate::media::outside::COPY_LABEL).map(|(_, p)| *p).max_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
    fx.hl.click(&mut fx.app, lower + egui::vec2(8.0, 6.0), PointerButton::Primary);
    copied(&mut fx);
    let text = note_text(&fx);
    assert!(text.starts_with("![[@Media/a.png|120]]") && text.contains("![[z_media/") && text.trim_end().ends_with("_tone.wav]]"), "only the sound: {text}");
    assert_eq!(std::fs::read_to_string(fx.dir.join("Player.md")).unwrap(), text, "the reading view saves the note");
    let upper = fx.hl.text.iter().filter(|(t, _)| t == crate::media::outside::COPY_LABEL).map(|(_, p)| *p).next().expect("the picture still has its button");
    fx.hl.click(&mut fx.app, upper + egui::vec2(8.0, 6.0), PointerButton::Primary);
    copied(&mut fx);
    let text = note_text(&fx);
    assert!(text.starts_with("![[z_media/") && text.contains("_a.png|120]]") && !text.contains("@Media"), "{text}");
    assert_eq!(std::fs::read_to_string(fx.dir.join("Player.md")).unwrap(), text);
    assert!(!said(&fx, crate::media::outside::COPY_LABEL), "nothing outside is left");
}

// spec: MEDIA-021
#[test]
fn the_note_menu_copies_all_outside_links_whose_files_are_there_and_reports_the_rest() {
    let o = Outside::new("copy-all");
    o.picture("a.png").fixture("tone.wav", "tone.wav");
    let address = o.address("2024/b.png");
    o.picture("2024/b.png");
    let note = format!("# Media\n\n![[@Media/a.png]] and ![[@Media/a.png|200]]\n\n[[@Media/tone.wav]]\n\n![b](<{}>)\n\n![[@Media/gone.mp4]] ![[@Nowhere/x.png]] [[Note]]\n", address.replace("%20", " "));
    let mut fx = window("o-copy-all", &note, o.library());
    fx.reading(false);
    fx.click_label("⋯");
    fx.click_label("Copy outside media into the vault");
    copied(&mut fx);
    let text = note_text(&fx);
    assert_eq!(text.matches("z_media/").count(), 4, "four links now point into the vault: {text}");
    assert!(text.contains("![[@Media/gone.mp4]] ![[@Nowhere/x.png]] [[Note]]"), "what is not there and what is no outside link stays: {text}");
    assert!(text.contains("_a.png]] and ![[z_media/") && text.contains("_a.png|200]]"), "{text}");
    assert!(text.contains("![b](<z_media/") && text.contains("_b.png>)"), "a file link keeps its form: {text}");
    let media = std::fs::read_dir(fx.dir.join("z_media")).map(|_| ()).is_ok();
    assert!(media);
    let copies: Vec<_> = walk_files(&fx.dir.join("z_media"));
    assert_eq!(copies.len(), 3, "the same file twice is one copy: {copies:?}");
    let toast = fx.app.toasts.iter().map(|(t, _)| t.clone()).find(|t| t.starts_with("Copied 3 files")).expect("the toast");
    assert!(toast.contains("@Media/gone.mp4") && toast.contains("@Nowhere/x.png"), "the files that are not there are named: {toast}");
    // nothing is left to copy but links that cannot be: the entry is off... and says so when it is asked anyway
    fx.app.copy_outside(std::path::Path::new("Player.md"), None);
    copied(&mut fx);
    assert!(toast_with(&fx, "No file was copied into the vault"), "{:?}", fx.app.toasts);
    assert_eq!(note_text(&fx), text, "nothing changed");
}

fn walk_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk_files(&p));
        } else {
            out.push(p);
        }
    }
    out
}

// spec: MEDIA-021
#[test]
fn copying_runs_one_at_a_time_says_when_there_is_nothing_to_copy_and_copes_with_a_note_that_closed_or_is_gone() {
    let o = Outside::new("copy-edges");
    o.picture("a.png");
    let rel = std::path::Path::new("Player.md");
    // a note without outside links: nothing to copy
    let mut plain = window("o-copy-none", "just text and [[Note]]\n", o.library());
    plain.app.copy_outside(rel, None);
    assert!(toast_with(&plain, "links no media from outside the vault"), "{:?}", plain.app.toasts);
    assert!(plain.app.outside_copy.is_none());
    plain.app.copy_outside(rel, Some("@Media/a.png".into()));
    assert!(plain.app.outside_copy.is_none(), "a target the note does not link is nothing to copy");
    // a second request while one runs is refused, the first goes on
    let mut fx = window("o-copy-edges", "![[@Media/a.png]]\n", o.library());
    fx.app.copy_outside(rel, None);
    assert!(fx.app.outside_copy.is_some());
    fx.app.copy_outside(rel, None);
    assert!(toast_with(&fx, "being copied into the vault already"));
    // the tab is closed before the job ends: no editor shows the note, so it is changed and saved here
    let tab = fx.app.tabbar.active;
    fx.app.close_tab(tab);
    copied(&mut fx);
    assert!(toast_with(&fx, "Copied 1 file into the vault"), "{:?}", fx.app.toasts);
    let saved = std::fs::read_to_string(fx.dir.join("Player.md")).unwrap();
    assert!(saved.starts_with("![[z_media/") && saved.ends_with("_a.png]]\n"), "{saved}");
    assert_eq!(walk_files(&fx.dir.join("z_media")).len(), 1);
    // the note is gone before the job ends (deleted or renamed): the file is copied, the link is left, and it is said
    let mut gone = window("o-copy-gone", "![[@Media/a.png]]\n", o.library());
    gone.app.copy_outside(rel, None);
    gone.app.notes.remove(rel);
    let tab = gone.app.tabbar.active;
    gone.app.close_tab(tab);
    copied(&mut gone);
    assert!(toast_with(&gone, "is not open, so its links were left as they are"), "{:?}", gone.app.toasts);
    assert_eq!(std::fs::read_to_string(gone.dir.join("Player.md")).unwrap(), "![[@Media/a.png]]\n");
    // a note that could not be read is not changed
    let mut bad = window("o-copy-unreadable", "![[@Media/a.png]]\n", o.library());
    bad.app.notes.get_mut(rel).unwrap().unreadable = Some("not text".into());
    bad.app.copy_outside(rel, None);
    assert!(toast_with(&bad, "could not be read, so it is not changed") && bad.app.outside_copy.is_none());
    // a copy that fails says why and leaves the note: the media folder is a file
    let mut fail = window("o-copy-fail", "![[@Media/a.png]]\n", o.library());
    std::fs::write(fail.dir.join("z_media"), b"in the way").unwrap();
    fail.app.copy_outside(rel, None);
    copied(&mut fail);
    assert!(toast_with(&fail, "Could not copy: @Media/a.png"), "{:?}", fail.app.toasts);
    assert_eq!(note_text(&fail), "![[@Media/a.png]]\n");
}

// spec: MEDIA-021
#[test]
fn an_outside_picture_that_is_not_there_has_no_copy_action_and_the_menu_entry_is_off_without_outside_links() {
    let o = Outside::new("copy-none");
    let mut fx = window("o-copy-missing", "![[@Media/gone.png]]\n", o.library());
    for reading in [false, true] {
        fx.reading(reading);
        assert!(!said(&fx, crate::media::outside::COPY_LABEL), "reading: {reading}: nothing to copy there");
    }
    let mut plain = window("o-copy-menu-off", "no media here\n", o.library());
    plain.reading(false);
    plain.click_label("⋯");
    plain.hl.frames(&mut plain.app, 2);
    let at = plain.hl.text.iter().find(|(t, _)| t == "Copy outside media into the vault").map(|(_, p)| *p).expect("the entry is in the menu");
    plain.hl.click(&mut plain.app, at + egui::vec2(8.0, 6.0), PointerButton::Primary);
    assert!(plain.app.outside_copy.is_none() && plain.app.toasts.iter().all(|(t, _)| !t.contains("Copying")), "it does nothing when it is off");
}
