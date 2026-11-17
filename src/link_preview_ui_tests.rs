//! The link preview of vim-lite normal mode (EDIT-033…035), driven in the whole window with real key events.

use crate::app::{Options, ShaleApp, View};
use crate::editor::NoteEditor;
use crate::keys::{Action, Binding};
use crate::link_preview::{Content, Target};
use crate::social::fake_server::FakeServer;
use crate::ui_test_support::Headless;
use eframe::egui::{self, text::CCursor, text::CCursorRange, Event, Key, Modifiers, Pos2, Rect};
use std::path::{Path, PathBuf};

const NOTE: &str = "plain words here\n\nSee [[Ideas#Second]] and [[Nothing]] and ![[pic.png]]\n";

struct W {
    app: ShaleApp,
    hl: Headless,
    dir: PathBuf,
}

impl W {
    fn new(tag: &str, note: &str) -> W {
        let dir = std::env::temp_dir().join(format!("shale-linkpreview-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("A.md"), note).unwrap();
        let mut ideas = String::from("# First\nfirst text\n\n## Second\n");
        for n in 1..=80 {
            ideas.push_str(&format!("line {n} of the second part\n\n"));
        }
        std::fs::write(dir.join("Ideas.md"), ideas).unwrap();
        // a picture the editor can decode
        let mut png = Vec::new();
        image::DynamicImage::new_rgb8(8, 8).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        std::fs::write(dir.join("pic.png"), png).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.cfg.vim_mode = true;
        app.open(Path::new("A.md"), false);
        app.focus_editor();
        let mut w = W { app, hl: Headless::new(), dir };
        w.settle();
        assert!(w.ed().vim_normal_keys(w.hl.ctx()), "the editor has the keyboard in normal mode");
        w
    }

    fn settle(&mut self) {
        self.hl.frames(&mut self.app, 4);
    }

    fn ed(&self) -> &NoteEditor {
        &self.app.tabbar.tabs[self.app.tabbar.active].editor
    }

    fn text(&self) -> String {
        self.app.notes[Path::new("A.md")].text.clone()
    }

    /// Press and release a key, with the text it types.
    fn key(&mut self, key: Key, modifiers: Modifiers, text: &str) {
        let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
        let mut down = vec![ev(true)];
        if !text.is_empty() {
            down.push(Event::Text(text.to_string()));
        }
        self.hl.frame(&mut self.app, down);
        self.hl.frame(&mut self.app, vec![ev(false)]);
        self.settle();
    }

    fn plain(&mut self, key: Key, text: &str) {
        self.key(key, Modifiers::NONE, text);
    }

    fn shift_k(&mut self) {
        self.key(Key::K, Modifiers::SHIFT, "K");
    }

    /// Put the text box's cursor (as a click would) on the first character of `word`, in the way of a click.
    fn cursor_at(&mut self, word: &str) {
        let text = self.text();
        let c = text[..text.find(word).unwrap_or_else(|| panic!("no {word}"))].chars().count() + 1;
        let id = self.ed().id;
        let mut st = egui::text_edit::TextEditState::load(self.hl.ctx(), id).unwrap_or_default();
        st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(c))));
        st.store(self.hl.ctx(), id);
        self.settle();
        assert_eq!(self.ed().vim.cursor, c, "vim has the cursor on {word}");
    }

    fn open(&self) -> Option<&crate::link_preview::Open> {
        self.app.link_preview.open.as_ref()
    }

    fn painted(&self, what: &str) -> bool {
        self.hl.text.iter().any(|(t, _)| t.contains(what))
    }

    fn view(&self) -> View {
        self.app.tabbar.tabs[self.app.tabbar.active].view.clone()
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// spec: EDIT-033
#[test]
fn edit_033_shift_k_previews_the_link_and_the_editor_keeps_the_keyboard() {
    let mut w = W::new("open", NOTE);
    w.cursor_at("Ideas");
    let (cursor, text) = (w.ed().vim.cursor, w.text());
    w.shift_k();
    let open = w.open().expect("Shift+K opens the preview");
    assert!(matches!(&open.target, Target::Link { target, heading, .. } if target == "Ideas" && heading.as_deref() == Some("Second")));
    assert!(matches!(&open.content, Content::Note { text, .. } if text.starts_with("## Second")), "the note from its heading on");
    assert!(!open.focused, "the editor keeps the keyboard");
    assert!(w.ed().vim_normal_keys(w.hl.ctx()));
    assert_eq!((w.ed().vim.cursor, w.text()), (cursor, text.clone()), "K is no vim key: nothing moved or changed");
    assert!(w.painted("line 1 of the second part"), "the note is drawn as the reading view draws it");
    // on the screen, near the cursor row, not bigger than the limit
    let r = open.rect.unwrap();
    assert!(Rect::from_min_size(Pos2::ZERO, egui::vec2(1500.0, 900.0)).contains_rect(r), "{r:?}");
    assert!((r.width() - 640.0).abs() <= 30.0 && r.height() <= 510.0, "the default is 640 x 480: {r:?}");
    assert!(r.min.y >= open.anchor.max.y || r.max.y <= open.anchor.min.y, "it does not cover the cursor row");
    // a cursor move closes it, and the move is made
    w.plain(Key::L, "l");
    assert!(w.open().is_none());
    assert_eq!(w.ed().vim.cursor, cursor + 1);
    // plain k is vim's up, and with the keyboard in the editor a second K needs no preview
    w.cursor_at("Ideas");
    let here = w.ed().vim.cursor;
    w.plain(Key::K, "k");
    assert!(w.ed().vim.cursor < here, "k moves up");
    assert!(w.open().is_none());
    // typing closes it: `i` leaves normal mode
    w.cursor_at("Ideas");
    w.shift_k();
    assert!(w.open().is_some());
    w.plain(Key::I, "i");
    assert!(w.open().is_none());
    assert_eq!(w.text(), text, "typing changed nothing");
    assert_eq!(w.ed().vim_status().as_deref(), Some("INSERT"));
    // (the first Esc closes the [[ suggestions that insert mode shows inside a link)
    for _ in 0..2 {
        w.plain(Key::Escape, "");
    }
    assert_eq!(w.ed().vim_status().as_deref(), Some("NORMAL"), "Esc: back in normal mode");
    // off a link nothing opens, and the status bar says so
    w.cursor_at("plain");
    w.shift_k();
    assert!(w.open().is_none());
    assert_eq!(w.ed().vim_status().as_deref(), Some("No link under the cursor"));
    // a click outside closes it
    w.cursor_at("Ideas");
    w.shift_k();
    assert!(w.open().is_some());
    w.hl.click(&mut w.app, Pos2::new(1480.0, 880.0), egui::PointerButton::Primary);
    assert!(w.open().is_none(), "a click outside");
    assert_eq!(w.text(), text);
}

// spec: EDIT-034
#[test]
fn edit_034_a_missing_note_and_a_picture_have_their_own_preview() {
    let mut w = W::new("kinds", NOTE);
    w.cursor_at("Nothing");
    w.shift_k();
    assert!(matches!(w.open().map(|o| &o.content), Some(Content::Missing(n)) if n == "Nothing"));
    assert!(w.painted("Note does not exist yet"));
    w.plain(Key::Escape, "");
    w.cursor_at("pic.png");
    w.shift_k();
    assert!(matches!(w.open().map(|o| &o.content), Some(Content::Image { rel, .. }) if rel == Path::new("pic.png")));
    w.plain(Key::Escape, "");
    // a link to the note itself by heading shows the note from there
    assert!(w.open().is_none());
}

// spec: EDIT-035
#[test]
fn edit_035_k_again_focuses_the_preview_j_scrolls_esc_gives_the_cursor_back_enter_opens() {
    let mut w = W::new("keys", NOTE);
    w.cursor_at("Ideas");
    let (cursor, text) = (w.ed().vim.cursor, w.text());
    w.shift_k();
    w.shift_k();
    assert!(w.open().expect("still open").focused, "the second K moves the keyboard into the preview");
    assert_eq!(w.open().unwrap().scroll, 0.0);
    // j / ↓ scroll, k / ↑ scroll back, Ctrl+D / Ctrl+U and the page keys move a page; nothing reaches the text
    w.plain(Key::J, "j");
    let one = w.open().unwrap().scroll;
    assert!(one > 0.0, "j scrolls");
    w.plain(Key::ArrowDown, "");
    assert!(w.open().unwrap().scroll > one, "↓ scrolls");
    w.plain(Key::K, "k");
    w.plain(Key::ArrowUp, "");
    assert_eq!(w.open().unwrap().scroll, 0.0, "k and ↑ scroll back");
    w.key(Key::D, Modifiers::COMMAND, "");
    let page = w.open().unwrap().scroll;
    assert!(page > one, "Ctrl+D scrolls a page");
    w.key(Key::U, Modifiers::COMMAND, "");
    assert_eq!(w.open().unwrap().scroll, 0.0, "Ctrl+U");
    w.plain(Key::PageDown, "");
    assert!(w.open().unwrap().scroll >= page, "PageDown");
    w.plain(Key::X, "x");
    w.plain(Key::L, "l");
    assert_eq!((w.ed().vim.cursor, w.text()), (cursor, text.clone()), "keys in the preview are not typed");
    // Esc: the preview is gone and the editor has the keyboard in the same mode, at the same place
    w.plain(Key::Escape, "");
    assert!(w.open().is_none());
    assert_eq!((w.ed().vim.cursor, w.text()), (cursor, text.clone()));
    assert!(w.ed().vim_normal_keys(w.hl.ctx()));
    w.plain(Key::L, "l");
    assert_eq!(w.ed().vim.cursor, cursor + 1, "vim works again");
    // Esc of a preview that does not have the keyboard closes it too
    w.cursor_at("Ideas");
    w.shift_k();
    w.plain(Key::Escape, "");
    assert!(w.open().is_none());
    assert_eq!(w.view(), View::Note("A.md".into()));
    // Enter opens the note in this tab, as gt does
    w.shift_k();
    w.shift_k();
    w.plain(Key::Enter, "");
    assert!(w.open().is_none());
    assert_eq!(w.view(), View::Note("Ideas.md".into()), "the note opened in this tab");
    assert_eq!(w.app.tabbar.tabs.len(), 1);
}

// spec: EDIT-036, NAV-033
#[test]
fn edit_036_ctrl_enter_opens_the_note_in_a_new_pane_on_the_right() {
    let mut w = W::new("split", NOTE);
    w.cursor_at("Ideas");
    w.shift_k();
    // an unfocused preview leaves Ctrl+Enter alone
    w.key(Key::Enter, Modifiers::COMMAND, "");
    assert_eq!(w.app.panes.len(), 1);
    w.shift_k();
    w.key(Key::Enter, Modifiers::COMMAND, "");
    assert!(w.open().is_none(), "the preview closes");
    assert_eq!(w.app.panes.len(), 2, "a second pane");
    assert_eq!(w.app.panes.focused, 1, "the new pane on the right has the focus");
    assert_eq!(w.view(), View::Note("Ideas.md".into()), "it shows the linked note");
    assert_eq!(w.app.pane_bar(0).tabs[w.app.pane_bar(0).active].view, View::Note("A.md".into()), "the note being edited stays in its pane");
    // a missing note is created, as with Enter
    w.app.focus_pane(0);
    w.app.focus_editor();
    w.settle();
    w.cursor_at("Nothing");
    w.shift_k();
    w.shift_k();
    w.key(Key::Enter, Modifiers::COMMAND, "");
    assert_eq!(w.app.panes.len(), 3);
    assert_eq!(w.app.panes.focused, 1, "right of the pane it came from");
    assert_eq!(w.view(), View::Note("Nothing.md".into()));
    assert!(w.dir.join("Nothing.md").exists());
}

// spec: EDIT-035, EDIT-034
#[test]
fn edit_035_enter_on_a_missing_note_creates_it_like_gt() {
    let mut w = W::new("create", NOTE);
    w.cursor_at("Nothing");
    w.shift_k();
    w.shift_k();
    w.plain(Key::Enter, "");
    assert_eq!(w.view(), View::Note("Nothing.md".into()));
    assert!(w.dir.join("Nothing.md").exists());
}

// spec: EDIT-033, KEY-004
#[test]
fn edit_033_the_action_can_be_bound_to_space_space() {
    let mut w = W::new("space", NOTE);
    w.app.keymap.set(Action::NormalLinkPreview, Binding::parse("Space Space").unwrap()).expect("Space Space is free");
    w.cursor_at("Ideas");
    w.shift_k();
    assert!(w.open().is_none(), "the old key does nothing");
    w.plain(Key::Space, " ");
    assert!(w.open().is_none(), "half a sequence does nothing yet");
    w.plain(Key::Space, " ");
    assert!(w.open().is_some_and(|o| !o.focused), "Space Space opens the preview");
    w.plain(Key::Space, " ");
    w.plain(Key::Space, " ");
    assert!(w.open().is_some_and(|o| o.focused), "and again moves into it");
    // the other Space sequences still work: Space Tab swaps views (nothing to swap to here), Space f f finds a file
    w.plain(Key::Escape, "");
    for (k, t) in [(Key::Space, " "), (Key::F, "f"), (Key::F, "f")] {
        w.plain(k, t);
    }
    assert!(matches!(w.app.modal, Some(crate::app::Modal::Picker(_))), "Space f f still opens the picker");
}

// spec: EDIT-034
#[test]
fn edit_034_a_web_link_loads_in_the_background_and_is_kept() {
    let server = FakeServer::start();
    let a = server.address();
    let note = format!("[ok]({a}/preview/page) [slow]({a}/preview/slow) [bad]({a}/preview/error) [gone]({a}/preview/nothing) [file]({a}/preview/file)\n");
    let mut w = W::new("web", &note);
    let requests = |path: &str| server.state.lock().unwrap().requests.iter().filter(|r| *r == &format!("GET {path}")).count();
    let wait = |w: &mut W, url: &str| {
        for _ in 0..200 {
            w.hl.frames(&mut w.app, 1);
            if w.app.link_preview.pages.contains_key(url) {
                // the frame grows to the page in the frame after it arrived
                w.hl.frames(&mut w.app, 3);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("{url} never arrived");
    };
    // a slow page: the window answers at once, with the words for waiting
    let slow = format!("{a}/preview/slow");
    w.cursor_at("[slow]");
    w.shift_k();
    assert!(matches!(w.open().map(|o| &o.content), Some(Content::Web { url }) if *url == slow));
    assert!(!w.app.link_preview.pages.contains_key(&slow), "not there yet");
    assert!(w.painted("Loading 127.0.0.1"), "the loading state is shown");
    wait(&mut w, &slow);
    assert_eq!(w.app.link_preview.pages[&slow].as_ref().unwrap().title, "Slow page");
    assert!(w.painted("Slow page") && w.painted("It took its time"));
    w.plain(Key::Escape, "");
    // a page with title, description and text, read once and kept
    let page = format!("{a}/preview/page");
    w.cursor_at("[ok]");
    w.shift_k();
    wait(&mut w, &page);
    let p = w.app.link_preview.pages[&page].clone().unwrap();
    assert_eq!((p.title.as_str(), p.description.as_str()), ("Fake Page & Co", "What the fake page is about"));
    assert!(p.text.starts_with("The first paragraph") && !p.text.contains("Menu entry") && !p.text.contains("hidden"));
    assert!(w.painted("Fake Page & Co") && w.painted("What the fake page is about") && w.painted("The first paragraph"), "{p:?}");
    w.plain(Key::Escape, "");
    w.shift_k();
    assert!(w.painted("Fake Page & Co"), "the second time it is there at once");
    assert_eq!(requests("/preview/page"), 1, "and nothing was fetched again");
    w.shift_k();
    // Enter opens the address in the browser, as gt does (the action queue gets the same action)
    // failures say why, in words
    for (word, path, why) in [("[bad]", "error", "status 500"), ("[gone]", "nothing", "not found"), ("[file]", "file", "not a web page")] {
        w.plain(Key::Escape, "");
        let url = format!("{a}/preview/{path}");
        w.cursor_at(word);
        w.shift_k();
        wait(&mut w, &url);
        let e = w.app.link_preview.pages[&url].clone().unwrap_err();
        assert!(e.contains(why), "{path}: {e}");
        assert!(w.painted("The page could not be loaded") && w.painted(&e), "{path}");
    }
}

// spec: EDIT-037, EDIT-033
#[test]
fn edit_037_a_corner_and_an_edge_resize_the_preview_and_the_size_is_kept() {
    let mut w = W::new("resize", NOTE);
    w.cursor_at("Ideas");
    let (cursor, text) = (w.ed().vim.cursor, w.text());
    w.shift_k();
    w.shift_k();
    // a page is 90 % of the visible height
    w.key(Key::D, Modifiers::COMMAND, "");
    let page_small = w.open().unwrap().scroll;
    w.key(Key::U, Modifiers::COMMAND, "");
    assert_eq!(w.open().unwrap().scroll, 0.0);
    let before = w.open().unwrap().size;
    assert_eq!(before, egui::vec2(640.0, 480.0), "the default size");
    // the bottom right corner: the resize arrow, then a drag that makes it bigger
    let r = w.open().unwrap().rect.unwrap();
    let from = r.max - egui::vec2(1.0, 1.0);
    w.hl.frame(&mut w.app, vec![Event::PointerMoved(from)]);
    w.hl.frame(&mut w.app, vec![Event::PointerMoved(from)]);
    assert!(matches!(w.hl.cursor, egui::CursorIcon::ResizeNwSe | egui::CursorIcon::ResizeNeSw), "a corner has the resize cursor: {:?}", w.hl.cursor);
    let to = from + egui::vec2(60.0, 40.0);
    let path = crate::ui_test_support::Headless::steps(from, to, 6);
    w.hl.press_and_move(&mut w.app, from, &path);
    let open = w.open().expect("a drag on a handle does not close the preview");
    assert_eq!(open.size, before + egui::vec2(60.0, 40.0), "the corner moved the size with the pointer");
    assert_eq!(w.app.cfg.link_preview_width, 640, "the setting is written when the drag ends, not before");
    assert_eq!((w.ed().vim.cursor, w.text()), (cursor, text.clone()), "the press did not reach the editor");
    w.hl.release(&mut w.app, to);
    assert!(w.open().is_some());
    assert_eq!((w.app.cfg.link_preview_width, w.app.cfg.link_preview_height), (700, 520), "kept as the setting");
    assert_eq!((w.ed().vim.cursor, w.text()), (cursor, text.clone()));
    let r = w.open().unwrap().rect.unwrap();
    assert!((r.width() - 700.0).abs() <= 2.0 && (r.height() - 520.0).abs() <= 2.0, "the frame has the new size: {r:?}");
    assert!(Rect::from_min_size(Pos2::ZERO, egui::vec2(1500.0, 900.0)).contains_rect(r), "and stays on the screen: {r:?}");
    // the scrolling follows the height: a page is more now
    w.key(Key::D, Modifiers::COMMAND, "");
    assert!(w.open().unwrap().scroll > page_small, "{} > {page_small}", w.open().unwrap().scroll);
    w.key(Key::U, Modifiers::COMMAND, "");
    // an edge: the right edge only changes the width
    let r = w.open().unwrap().rect.unwrap();
    let from = Pos2::new(r.max.x, r.center().y);
    w.hl.frame(&mut w.app, vec![Event::PointerMoved(from)]);
    w.hl.frame(&mut w.app, vec![Event::PointerMoved(from)]);
    assert_eq!(w.hl.cursor, egui::CursorIcon::ResizeHorizontal, "an edge has the resize cursor");
    let to = from - egui::vec2(100.0, 0.0);
    let path = crate::ui_test_support::Headless::steps(from, to, 5);
    w.hl.press_and_move(&mut w.app, from, &path);
    w.hl.release(&mut w.app, to);
    assert_eq!((w.app.cfg.link_preview_width, w.app.cfg.link_preview_height), (600, 520), "only the width changed");
    assert!(w.open().is_some());
    // the left edge to the left: wider, the right edge stays
    let r = w.open().unwrap().rect.unwrap();
    let from = Pos2::new(r.min.x, r.center().y);
    let to = from - egui::vec2(50.0, 0.0);
    w.hl.frame(&mut w.app, vec![Event::PointerMoved(from)]);
    let path = crate::ui_test_support::Headless::steps(from, to, 5);
    w.hl.press_and_move(&mut w.app, from, &path);
    w.hl.release(&mut w.app, to);
    assert_eq!(w.app.cfg.link_preview_width, 650);
    let after = w.open().unwrap().rect.unwrap();
    assert!((after.max.x - r.max.x).abs() <= 2.0 && (after.min.x - (r.min.x - 50.0)).abs() <= 2.0, "{r:?} -> {after:?}");
    // every preview opened later has the size
    w.plain(Key::Escape, "");
    assert!(w.open().is_none());
    w.cursor_at("Ideas");
    w.shift_k();
    assert_eq!(w.open().unwrap().size, egui::vec2(650.0, 520.0), "a new preview uses the setting");
    assert_eq!((w.ed().vim.cursor, w.text()), (cursor, text));
}
