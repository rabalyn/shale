//! The feed view inside the whole window: tabs, titles, scroll positions, restoring tabs.
//! The window runs headless (`ShaleApp::run_frame`); controls are clicked with real pointer
//! events at the places the view reports in `SocialState::rects` and `ShaleApp::tab_rects`.

use super::state::Timeline;
use super::tests::post;
use super::{demo, FeedRef, Post};
use crate::app::{Options, ShaleApp, View};
use eframe::egui::{self, Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect, Vec2};
use std::path::PathBuf;

pub(crate) struct W {
    pub app: ShaleApp,
    pub ctx: egui::Context,
    t: f64,
    pub dir: PathBuf,
}

pub(crate) fn feed_posts(prefix: &str, n: usize) -> Vec<Post> {
    (0..n).map(|i| post(&format!("{prefix}{i}"), "bob", 1000 - i as i64, None)).collect()
}

pub(crate) fn home() -> FeedRef {
    FeedRef { account: demo::account().account.id, feed: "home".into() }
}

pub(crate) fn list() -> FeedRef {
    FeedRef { account: demo::account().account.id, feed: "list:1".into() }
}

impl W {
    /// A window on a small vault with the demo account; both feeds hold 40 short posts.
    pub fn new(tag: &str) -> W {
        let dir = std::env::temp_dir().join(format!("shale-socialui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Welcome.md"), "# Welcome\n").unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        let entry = demo::account();
        app.cfg.social_accounts = vec![entry.clone()];
        app.social.load_demo(&entry);
        for (f, p) in [("home", "h"), ("list:1", "l")] {
            app.social.timelines.insert((entry.account.id.clone(), f.into()), Timeline { posts: feed_posts(p, 40), cursor: None, loading: false, loaded: true, ..Default::default() });
        }
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut w = W { app, ctx, t: 0.0, dir };
        w.app.show_view(View::Social(None), false);
        w.frames(4);
        w
    }

    pub fn frame(&mut self, events: Vec<Event>) {
        self.t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1500.0, 900.0))), time: Some(self.t), events, ..Default::default() };
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(raw, |ui| app.run_frame(ui));
        out.textures_delta.clear();
    }

    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![]);
        }
    }

    pub fn rect(&self, key: &str) -> Rect {
        self.app.social.rects.iter().rev().find(|r| r.0 == key).unwrap_or_else(|| panic!("no control {key:?}: {:?}", self.app.social.rects.iter().map(|r| &r.0).collect::<Vec<_>>())).1
    }

    pub fn click_at(&mut self, p: Pos2) {
        let b = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        self.frame(vec![Event::PointerMoved(p)]);
        self.frame(vec![b(true)]);
        self.frame(vec![b(false)]);
        self.frames(3);
    }

    pub fn click(&mut self, key: &str) {
        let p = self.rect(key).center();
        self.click_at(p);
    }

    pub fn click_tab(&mut self, i: usize) {
        let p = self.app.tab_rects[i].0.center();
        self.click_at(p);
    }

    /// Turn the mouse wheel over the feed (`dy` points down) and let the scrolling settle.
    pub fn scroll(&mut self, dy: f32) {
        let p = Pos2::new(800.0, 500.0);
        self.frame(vec![Event::PointerMoved(p)]);
        self.frame(vec![Event::MouseWheel { unit: MouseWheelUnit::Point, delta: Vec2::new(0.0, -dy), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: Modifiers::NONE }]);
        self.frames(12);
    }

    pub fn view(&self, tab: usize) -> View {
        self.app.tabs[tab].view.clone()
    }

    pub fn title(&self, tab: usize) -> String {
        self.app.social_title(match &self.app.tabs[tab].view {
            View::Social(f) => f.as_ref(),
            _ => None,
        })
    }

    /// The screen y of the post at `index` of the list shown.
    pub fn y(&self, index: usize) -> f32 {
        self.rect(&format!("save:{index}")).min.y
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// spec: SOC-021, SOC-022, SOC-023
#[test]
fn two_feeds_open_in_two_tabs_each_with_its_own_scroll_and_title() {
    let mut w = W::new("tabs");
    assert_eq!(w.app.tabs.len(), 1);
    assert_eq!(w.view(0), View::Social(Some(home())), "the tab names the feed it shows once the feeds are known");
    assert_eq!(w.title(0), "Mastodon · Home");
    let top = w.y(0);
    w.scroll(700.0);
    let home_scrolled = w.y(0);
    assert!(home_scrolled < top - 300.0, "the list scrolled: {top} -> {home_scrolled}");
    // "New tab" on the entry of another feed in the feed picker
    w.click("feed-combo");
    w.click("feed-new-tab:list:1");
    assert_eq!((w.app.tabs.len(), w.app.active), (2, 1));
    assert_eq!(w.view(1), View::Social(Some(list())));
    assert_eq!(w.title(1), "Mastodon · List: Rust people");
    assert!((w.y(0) - top).abs() < 1.0, "a new tab starts at the top");
    w.scroll(300.0);
    let list_scrolled = w.y(0);
    assert!(list_scrolled < top - 100.0);
    // back to the first tab: its scroll position is where it was left, nothing was loaded again
    w.click_tab(0);
    assert_eq!(w.app.active, 0);
    w.frames(3);
    assert!((w.y(0) - home_scrolled).abs() < 1.0, "{} vs {home_scrolled}", w.y(0));
    w.click_tab(1);
    w.frames(3);
    assert!((w.y(0) - list_scrolled).abs() < 1.0);
    assert!(w.app.social.timelines.values().all(|t| !t.loading && t.posts.len() == 40), "switching tabs fetched nothing");
    // the same feed in a third tab: its own scroll
    w.click_tab(0);
    w.click("feed-combo");
    w.click("feed-new-tab:home");
    assert_eq!((w.app.tabs.len(), w.app.active), (3, 2));
    assert_eq!(w.view(2), w.view(0));
    assert!((w.y(0) - top).abs() < 1.0, "the same feed in another tab starts at the top");
    w.click_tab(0);
    w.frames(3);
    assert!((w.y(0) - home_scrolled).abs() < 1.0, "the first tab keeps its place");
}

// spec: SOC-022
#[test]
fn the_feeds_button_shows_an_existing_feeds_tab_and_a_new_empty_tab_becomes_a_second_one() {
    let mut w = W::new("ribbon");
    w.app.new_tab(View::Note(PathBuf::from("Welcome.md")));
    w.app.show_view(View::Social(None), false);
    assert_eq!(w.app.active, 0, "the existing Feeds tab is shown");
    assert_eq!(w.app.tabs.len(), 2);
    // the (+) button: an empty tab, then Feeds in it
    w.app.new_tab(View::Empty);
    w.app.show_view(View::Social(None), false);
    assert_eq!((w.app.tabs.len(), w.app.active), (3, 2));
    w.frames(3);
    assert_eq!(w.view(2), View::Social(Some(home())), "the last feed is shown");
}

// spec: SOC-021
#[test]
fn choosing_another_feed_is_a_step_in_the_tab_history() {
    let mut w = W::new("history");
    w.click("feed-combo");
    w.click("feed:list:1");
    assert_eq!((w.app.tabs.len(), w.view(0)), (1, View::Social(Some(list()))));
    assert_eq!(w.title(0), "Mastodon · List: Rust people");
    w.app.go_back();
    assert_eq!(w.view(0), View::Social(Some(home())));
    w.app.go_forward();
    assert_eq!(w.view(0), View::Social(Some(list())));
    // Shift+click on an entry opens it in a new tab
    w.click("feed-combo");
    w.frame(vec![Event::ModifiersChanged(Modifiers::SHIFT)]);
    w.click("feed:home");
    w.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
    assert_eq!((w.app.tabs.len(), w.view(1)), (2, View::Social(Some(home()))));
    // and so does a middle click
    w.click_tab(0);
    w.click("feed-combo");
    let p = w.rect("feed:home").center();
    let b = |pressed| Event::PointerButton { pos: p, button: PointerButton::Middle, pressed, modifiers: Modifiers::NONE };
    w.frame(vec![Event::PointerMoved(p)]);
    w.frame(vec![b(true)]);
    w.frame(vec![b(false)]);
    w.frames(3);
    assert_eq!(w.app.tabs.len(), 3);
}

// spec: SOC-024
#[test]
fn open_feed_tabs_come_back_and_an_old_state_file_still_loads() {
    let mut w = W::new("restore");
    w.app.open(std::path::Path::new("Welcome.md"), true);
    w.click_tab(0);
    w.click("feed-combo");
    w.click("feed-new-tab:list:1");
    assert_eq!(w.app.vdata.state.open_feeds, vec![home(), list()]);
    assert_eq!(w.app.vdata.state.open_files, vec!["Welcome.md".to_string()]);
    w.app.vdata.save().unwrap();
    let saved = std::fs::read_to_string(w.dir.join(".shale/state.json")).unwrap();
    assert!(saved.contains("open_feeds") && saved.contains("list:1"), "{saved}");
    // the next start: note tab first, then the feeds
    let mut again = ShaleApp::build(Options { screenshot: None, vault: Some(w.dir.clone()), view: None, keys: None }, false);
    again.cfg.social_accounts = vec![demo::account()];
    again.open_vault(&w.dir.clone());
    assert_eq!(again.tabs.iter().map(|t| t.view.clone()).collect::<Vec<_>>(), vec![View::Note(PathBuf::from("Welcome.md")), View::Social(Some(home())), View::Social(Some(list()))]);
    // an old state file (no open_feeds) and a feed of an account that is gone
    for (json, feeds) in [(r#"{"version": 1, "open_files": ["Welcome.md"]}"#, 0), (r#"{"version": 1, "open_files": ["Welcome.md"], "open_feeds": [{"account": "gone", "feed": "home"}]}"#, 0)] {
        crate::fsutil::write_atomic(&w.dir.join(".shale/state.json"), json.as_bytes()).unwrap();
        let mut old = ShaleApp::build(Options { screenshot: None, vault: Some(w.dir.clone()), view: None, keys: None }, false);
        old.cfg.social_accounts = vec![demo::account()];
        old.open_vault(&w.dir.clone());
        assert_eq!(old.tabs.len(), 1 + feeds, "{json}");
        assert!(matches!(old.tabs[0].view, View::Note(_)));
    }
}
