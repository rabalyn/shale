//! The feed view inside the whole window: tabs, titles, scroll positions, restoring tabs.
//! The window runs headless (`ShaleApp::run_frame`); controls are clicked with real pointer
//! events at the places the view reports in `SocialState::rects` and `ShaleApp::tab_rects`.

use super::state::{post_key, Source, Timeline};
use super::tests::post;
use super::{demo, Feed, FeedRef, Fresh, Page, Post, Since};
use crate::app::{Options, ShaleApp, View};
use eframe::egui::{self, Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect, Vec2};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A pretend server: what the next checks find (per feed) and what a first page holds.
#[derive(Default)]
pub(crate) struct Script {
    pub newer: Mutex<Vec<(String, Fresh)>>,
    pub first_page: Mutex<Option<Page>>,
    /// what fetching a post by id gives (SOC-039); missing ids fail
    pub posts: Mutex<Vec<Post>>,
    pub post_calls: Mutex<Vec<String>>,
}

impl Source for Script {
    fn page(&self, _feed: &Feed, cursor: Option<&str>) -> Result<Page, String> {
        Ok(if cursor.is_none() { self.first_page.lock().unwrap().clone().unwrap_or_default() } else { Page::default() })
    }
    fn post(&self, id: &str) -> Result<Post, String> {
        self.post_calls.lock().unwrap().push(id.to_string());
        self.posts.lock().unwrap().iter().find(|p| p.id == id).cloned().ok_or_else(|| "the server does not know this post".to_string())
    }
    fn newer(&self, feed: &Feed, _since: &Since) -> Result<Fresh, String> {
        let mut q = self.newer.lock().unwrap();
        Ok(match q.iter().position(|(f, _)| *f == feed.id) {
            Some(i) => q.remove(i).1,
            None => Fresh::default(),
        })
    }
}

pub(crate) struct W {
    pub script: Arc<Script>,
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
            app.social.timelines.insert((entry.account.id.clone(), f.into()), Timeline { posts: feed_posts(p, 40), loaded: true, ..Default::default() });
        }
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let script = Arc::new(Script::default());
        app.social.source = Some(script.clone());
        let mut w = W { script, app, ctx, t: 0.0, dir };
        // the window starts with the Feeds tab only (the Feeds button would add one to the note tab)
        w.app.tabbar.tabs.clear();
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
        let p = self.app.tabbar.tab_rects[i].0.center();
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
        self.app.tabbar.tabs[tab].view.clone()
    }

    pub fn title(&self, tab: usize) -> String {
        self.app.social_title(match &self.app.tabbar.tabs[tab].view {
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

// spec: PERF-006
#[test]
fn perf_006_posts_far_from_the_view_are_not_laid_out_again_and_come_back_when_scrolled_to() {
    let mut w = W::new("farposts");
    let acct = home().account;
    w.app.social.timelines.get_mut(&(acct, "home".to_string())).unwrap().posts = recent("p", 400);
    w.frames(6);
    let drawn = |w: &W| w.app.social.rects.iter().filter(|(k, _)| k.starts_with("save:")).count();
    assert_eq!(w.app.social.post_rects.len(), 400, "every post has its place");
    assert!(drawn(&w) > 0 && drawn(&w) < 40, "{} of 400 posts laid out", drawn(&w));
    let before = w.app.social.post_rects[0].1.min.y;
    // the heights of the posts far away are the ones measured: scrolling there draws them
    w.scroll(6000.0);
    w.frames(6);
    assert!(w.app.social.rects.iter().any(|(k, _)| k.starts_with("save:") && k[5..].parse::<usize>().is_ok_and(|i| i > 20)), "posts further down are drawn now");
    assert!(drawn(&w) < 40 && w.app.social.post_rects[0].1.min.y < before - 1000.0, "{} drawn", drawn(&w));
    // the list is as long as before: the last post is at its place, 143 points for each post before it
    assert!((w.app.social.post_rects[399].1.min.y - w.app.social.post_rects[0].1.min.y - 399.0 * 143.0).abs() < 2.0);
}

// spec: SOC-021, SOC-022, SOC-023
#[test]
fn two_feeds_open_in_two_tabs_each_with_its_own_scroll_and_title() {
    let mut w = W::new("tabs");
    assert_eq!(w.app.tabbar.tabs.len(), 1);
    assert_eq!(w.view(0), View::Social(Some(home())), "the tab names the feed it shows once the feeds are known");
    assert_eq!(w.title(0), "Mastodon · Home");
    let top = w.y(0);
    w.scroll(700.0);
    let home_scrolled = w.y(0);
    assert!(home_scrolled < top - 300.0, "the list scrolled: {top} -> {home_scrolled}");
    // "New tab" on the entry of another feed in the feed picker
    w.click("feed-combo");
    w.click("feed-new-tab:list:1");
    assert_eq!((w.app.tabbar.tabs.len(), w.app.tabbar.active), (2, 1));
    assert_eq!(w.view(1), View::Social(Some(list())));
    assert_eq!(w.title(1), "Mastodon · List: Rust people");
    assert!((w.y(0) - top).abs() < 1.0, "a new tab starts at the top");
    w.scroll(300.0);
    let list_scrolled = w.y(0);
    assert!(list_scrolled < top - 100.0);
    // back to the first tab: its scroll position is where it was left, nothing was loaded again
    w.click_tab(0);
    assert_eq!(w.app.tabbar.active, 0);
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
    assert_eq!((w.app.tabbar.tabs.len(), w.app.tabbar.active), (3, 2));
    assert_eq!(w.view(2), w.view(0));
    assert!((w.y(0) - top).abs() < 1.0, "the same feed in another tab starts at the top");
    w.click_tab(0);
    w.frames(3);
    assert!((w.y(0) - home_scrolled).abs() < 1.0, "the first tab keeps its place");
}

// spec: SOC-022
#[test]
fn the_feeds_button_always_opens_a_new_feeds_tab_and_an_empty_tab_is_filled() {
    let mut w = W::new("ribbon");
    w.app.tabbar.new_tab(View::Note(PathBuf::from("Welcome.md")));
    // the ribbon button, clicked twice: two more Feeds tabs, none focused or replaced
    for n in [3, 4] {
        w.app.show_view(View::Social(None), false);
        assert_eq!((w.app.tabbar.tabs.len(), w.app.tabbar.active), (n, n - 1));
        w.frames(3);
        assert_eq!(w.view(n - 1), View::Social(Some(home())), "the new tab shows the feed set-up");
    }
    assert!(matches!(w.view(1), View::Note(_)), "the note tab stays");
    // the (+) button: an empty tab, then Feeds in it, makes no extra tab
    w.app.tabbar.new_tab(View::Empty);
    w.app.show_view(View::Social(None), false);
    assert_eq!((w.app.tabbar.tabs.len(), w.app.tabbar.active), (5, 4));
}

// spec: SOC-021
#[test]
fn choosing_another_feed_is_a_step_in_the_tab_history() {
    let mut w = W::new("history");
    w.click("feed-combo");
    w.click("feed:list:1");
    assert_eq!((w.app.tabbar.tabs.len(), w.view(0)), (1, View::Social(Some(list()))));
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
    assert_eq!((w.app.tabbar.tabs.len(), w.view(1)), (2, View::Social(Some(home()))));
    // and so does a middle click
    w.click_tab(0);
    w.click("feed-combo");
    let p = w.rect("feed:home").center();
    let b = |pressed| Event::PointerButton { pos: p, button: PointerButton::Middle, pressed, modifiers: Modifiers::NONE };
    w.frame(vec![Event::PointerMoved(p)]);
    w.frame(vec![b(true)]);
    w.frame(vec![b(false)]);
    w.frames(3);
    assert_eq!(w.app.tabbar.tabs.len(), 3);
}

/// Wait (up to 2 s) until a background job of the feeds has delivered, as `until` sees it.
fn wait_for(w: &mut W, until: impl Fn(&W) -> bool) {
    for _ in 0..200 {
        w.frame(vec![]);
        if until(w) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the background job did not finish");
}

fn recent(prefix: &str, n: usize) -> Vec<Post> {
    let now = crate::calendar::time::now();
    (0..n).map(|i| post(&format!("{prefix}{i}"), "bob", now - 3600 * (i as i64 + 1), None)).collect()
}

// spec: SOC-030
#[test]
fn posts_that_pages_and_checks_bring_in_are_searchable() {
    let mut w = W::new("indexing");
    let entry = demo::account();
    let feed = Feed { id: "home".into(), title: "Home".into() };
    assert_eq!(w.app.social.index.count(None), 0, "the posts a test puts into a timeline directly were not loaded");
    // the first page of the feed
    *w.script.first_page.lock().unwrap() = Some(Page { posts: recent("first", 3), ..Default::default() });
    w.app.social.reload(&w.ctx, &entry, &feed);
    wait_for(&mut w, |w| w.app.social.index.count(None) == 3);
    assert_eq!(w.app.social.index.search("first1", 5)[0].feed, "home");
    // a check: the posts that wait are searchable before they are shown
    let newer = recent("newer", 2);
    w.script.newer.lock().unwrap().push(("home".into(), Fresh { posts: newer.clone(), ..Default::default() }));
    assert!(w.app.social.check(&w.ctx, &entry, &feed));
    wait_for(&mut w, |w| w.app.social.index.count(None) == 5);
    assert_eq!(w.app.social.waiting_count(&entry.account.id, "home"), 2);
    assert_eq!(w.app.social.index.search("newer0", 5).len(), 1);
    // a post that waits and is found again is not a second entry; the page of a gap is indexed too
    w.app.social.index.add(&entry.account.id, "home", &newer);
    assert_eq!(w.app.social.index.count(None), 5);
}

// spec: SOC-033
#[test]
fn removing_the_account_in_the_dialog_drops_its_posts_from_the_search_but_adding_it_again_keeps_them() {
    let mut w = W::new("remove-index");
    let entry = demo::account();
    w.app.social.index.add(&entry.account.id, "home", &recent("gone", 2));
    assert_eq!(w.app.social.index.search("gone1", 5).len(), 1);
    // the same account signed in again (a new secret): its posts stay
    assert!(!w.app.social_dialog_action(crate::social::accounts_ui::DialogAction::Added(entry.clone())));
    assert_eq!(w.app.social.index.count(None), 2);
    w.app.social_dialog_action(crate::social::accounts_ui::DialogAction::Remove(entry.account.id.clone()));
    assert!(w.app.social.index.search("gone1", 5).is_empty());
    assert_eq!(w.app.social.index.count(None), 0);
}

// spec: SOC-035, SOC-036
#[test]
fn a_post_found_in_the_sidebar_search_opens_in_its_feed_scrolled_to_the_post() {
    let mut w = W::new("hit");
    let (acct, list_key) = (home().account, (home().account, "list:1".to_string()));
    let now = crate::calendar::time::now();
    for (f, p) in [("home", "h"), ("list:1", "l")] {
        w.app.social.timelines.get_mut(&(acct.clone(), f.to_string())).unwrap().posts = recent(p, 40);
    }
    w.app.side.left = crate::app::LeftPanel::Search;
    let search = |w: &mut W, q: &str| {
        w.app.search.query = q.into();
        w.frames(3);
    };
    // 1: a post the feed holds (in the middle of its list)
    let held = w.app.social.timelines[&list_key].posts[30].clone();
    w.app.social.index.add(&acct, "list:1", std::slice::from_ref(&held));
    search(&mut w, "l30");
    assert_eq!(w.app.social.hit_rects.len(), 1, "one post row in the sidebar");
    let p = w.app.social.hit_rects[0].1.center();
    w.click_at(p);
    assert_eq!((w.app.tabbar.tabs.len(), w.app.tabbar.active), (2, 1), "a new Feeds tab");
    assert_eq!(w.view(1), View::Social(Some(list())));
    assert_eq!(w.view(0), View::Social(Some(home())), "the first tab is left as it was");
    w.frames(4);
    let at = |w: &W, k: &str| w.app.social.post_rects.iter().find(|r| r.0 == k).unwrap_or_else(|| panic!("post {k} not shown")).1.min.y - w.app.social.list_rect.min.y;
    assert!(at(&w, &post_key(&held)).abs() < 2.0, "the post is at the top of the list, not at {}", at(&w, &post_key(&held)));
    assert_eq!(w.app.social.timelines[&list_key].posts.len(), 40, "nothing was added: the feed had it");
    assert!(w.app.social.focus.is_none());
    // 2: a post the feed no longer holds goes into the list where its date puts it
    let mut gone = post("zebra", "bob", now - 3600 * 11 - 1800, None);
    gone.text_markdown = "an unusual zebra".into();
    w.app.social.index.add(&acct, "list:1", &[gone.clone()]);
    search(&mut w, "zebra");
    let p = w.app.social.hit_rects[0].1.center();
    w.click_at(p);
    w.frames(4);
    let posts = &w.app.social.timelines[&list_key].posts;
    assert_eq!((posts.len(), posts[11].id.as_str(), posts[10].id.as_str()), (41, "zebra", "l10"));
    assert_eq!(w.app.tabbar.tabs.len(), 3);
    assert!(at(&w, &post_key(&gone)).abs() < 2.0, "shown at the top of the list");
    // 3: older than the whole list, which has more pages: it goes to the end, with the pages between as a gap
    w.app.social.timelines.get_mut(&list_key).unwrap().cursor = Some("c".into());
    let old = post("ancient", "bob", now - 3600 * 100, None);
    w.app.social.index.add(&acct, "list:1", &[Post { text_markdown: "something ancient".into(), ..old.clone() }]);
    search(&mut w, "something ancient");
    let p = w.app.social.hit_rects[0].1.center();
    w.click_at(p);
    w.frames(4);
    let t = &w.app.social.timelines[&list_key];
    assert_eq!(t.posts.last().map(|p| p.id.as_str()), Some("ancient"));
    assert_eq!(t.cursor, None);
    assert_eq!(t.gaps.last().map(|g| (g.above.as_str(), g.cursor.as_str())), Some((post_key(&t.posts[t.posts.len() - 2]).as_str(), "c")));
    // 4: a post that waits behind the button is shown with the others when its hit is opened
    let waiting = post("waits", "bob", now - 10, None);
    w.app.social.timelines.get_mut(&list_key).unwrap().waiting = vec![waiting.clone()];
    w.app.social.index.add(&acct, "list:1", &[Post { text_markdown: "waits for you".into(), ..waiting }]);
    search(&mut w, "waits for you");
    let p = w.app.social.hit_rects[0].1.center();
    w.click_at(p);
    w.frames(4);
    let t = &w.app.social.timelines[&list_key];
    assert!(t.waiting.is_empty() && t.posts[0].id == "waits");
    // 5: a feed that is gone: the account's other feed is shown, with the post in it
    let mut elsewhere = post("elsewhere", "bob", now - 3600 * 5 - 1800, None);
    elsewhere.text_markdown = "from a vanished feed".into();
    w.app.social.index.add(&acct, "list:vanished", &[elsewhere]);
    search(&mut w, "vanished feed");
    let p = w.app.social.hit_rects[0].1.center();
    w.click_at(p);
    w.frames(4);
    let n = w.app.tabbar.tabs.len();
    assert!(matches!(w.view(n - 1), View::Social(Some(f)) if f.account == acct), "{:?}", w.view(n - 1));
    let shown = w.app.tabbar.tabs[n - 1].view.clone();
    let View::Social(Some(shown)) = shown else { panic!() };
    assert!(w.app.social.timelines[&(acct.clone(), shown.feed)].posts.iter().any(|p| p.id == "elsewhere"));
    // 6: the account is gone: nothing opens, the user is told
    let hit = w.app.social.index.search("zebra", 1).remove(0);
    let tabs = w.app.tabbar.tabs.len();
    w.app.open_post_hit(&crate::social::post_index::PostHit { account: "mastodon:nobody".into(), ..hit });
    assert_eq!(w.app.tabbar.tabs.len(), tabs);
}

// spec: SOC-035
#[test]
fn a_hit_waits_for_the_feed_to_load_and_is_dropped_when_that_fails() {
    let mut w = W::new("hit-wait");
    let acct = home().account;
    let key = (acct.clone(), "home".to_string());
    let hit_post = post("late", "bob", crate::calendar::time::now() - 60, None);
    w.app.social.timelines.get_mut(&key).unwrap().loaded = false;
    w.app.social.focus = Some(crate::social::state::Focus { tab: 7, account: acct.clone(), post: hit_post.clone() });
    assert_eq!(w.app.social.take_focus(8, &key), None, "another tab is not asked");
    assert_eq!(w.app.social.take_focus(7, &key), None, "the list is still loading");
    assert!(w.app.social.focus.is_some());
    w.app.social.timelines.get_mut(&key).unwrap().error = Some("offline".into());
    assert_eq!(w.app.social.take_focus(7, &key), None);
    assert!(w.app.social.focus.is_none(), "a feed that cannot be loaded drops the hit");
    w.app.social.timelines.get_mut(&key).unwrap().loaded = true;
    w.app.social.focus = Some(crate::social::state::Focus { tab: 7, account: acct, post: hit_post.clone() });
    assert_eq!(w.app.social.take_focus(7, &key), Some(post_key(&hit_post)));
    assert_eq!(w.app.social.timelines[&key].posts[0].id, "late");
}

// spec: SOC-024
#[test]
fn open_feed_tabs_come_back_and_an_old_state_file_still_loads() {
    let mut w = W::new("restore");
    w.app.open(std::path::Path::new("Welcome.md"), true);
    w.click_tab(0);
    w.click("feed-combo");
    w.click("feed-new-tab:list:1");
    w.app.vdata.save().unwrap();
    let saved = std::fs::read_to_string(w.dir.join(".shale/state.json")).unwrap();
    assert!(saved.contains("\"tabs\"") && saved.contains("list:1"), "{saved}");
    // the next start: note tab first, then the feeds
    let mut again = ShaleApp::build(Options { screenshot: None, vault: Some(w.dir.clone()), view: None, keys: None }, false);
    again.cfg.social_accounts = vec![demo::account()];
    again.open_vault(&w.dir.clone());
    assert_eq!(again.tabbar.tabs.iter().map(|t| t.view.clone()).collect::<Vec<_>>(), vec![View::Social(Some(home())), View::Note(PathBuf::from("Welcome.md")), View::Social(Some(list()))], "every tab comes back in its place");
    // an old state file (no open_feeds) and a feed of an account that is gone
    for (json, feeds) in [(r#"{"version": 1, "open_files": ["Welcome.md"]}"#, 0), (r#"{"version": 1, "open_files": ["Welcome.md"], "open_feeds": [{"account": "gone", "feed": "home"}]}"#, 0)] {
        crate::fsutil::write_atomic(&w.dir.join(".shale/state.json"), json.as_bytes()).unwrap();
        let mut old = ShaleApp::build(Options { screenshot: None, vault: Some(w.dir.clone()), view: None, keys: None }, false);
        old.cfg.social_accounts = vec![demo::account()];
        old.open_vault(&w.dir.clone());
        assert_eq!(old.tabbar.tabs.len(), 1 + feeds, "{json}");
        assert!(matches!(old.tabbar.tabs[0].view, View::Note(_)));
    }
}

// spec: NAV-026
#[test]
fn notes_a_drawing_and_feeds_tabs_come_back_in_one_order_with_their_pins() {
    let mut w = W::new("mixed-restore");
    std::fs::write(w.dir.join("Ideas.md"), "# Ideas\n").unwrap();
    std::fs::write(w.dir.join("Sketch.excalidraw"), crate::drawing::SAMPLE_SCENE).unwrap();
    // a vault opened again knows the new files (and starts with the Welcome note only)
    let dir = w.dir.clone();
    w.app.open_vault(&dir);
    w.app.open(std::path::Path::new("Sketch.excalidraw"), true);
    w.app.open_feed(home(), true);
    w.app.open_feed(list(), true);
    w.app.open(std::path::Path::new("Ideas.md"), true);
    // tabs: Welcome, Sketch, home, list, Ideas; pin the drawing and the second Feeds tab
    assert_eq!(w.app.tabbar.tabs.len(), 5);
    w.app.set_tab_pinned(1, true);
    w.app.set_tab_pinned(3, true);
    let order: Vec<View> = w.app.tabbar.tabs.iter().map(|t| t.view.clone()).collect();
    assert_eq!(order[..2], [View::Drawing(PathBuf::from("Sketch.excalidraw")), View::Social(Some(list()))]);
    w.app.vdata.save().unwrap();
    let mut again = ShaleApp::build(Options { screenshot: None, vault: Some(w.dir.clone()), view: None, keys: None }, false);
    again.cfg.social_accounts = vec![demo::account()];
    again.open_vault(&w.dir.clone());
    let got: Vec<(View, bool)> = again.tabbar.tabs.iter().map(|t| (t.view.clone(), t.pinned)).collect();
    let want: Vec<(View, bool)> = w.app.tabbar.tabs.iter().map(|t| (t.view.clone(), t.pinned)).collect();
    assert_eq!(got, want);
    assert_eq!(got.iter().filter(|(_, p)| *p).count(), 2);
}

// ---- new posts

fn fresh(ids: &[&str]) -> Fresh {
    Fresh { posts: ids.iter().map(|i| post(i, "carol", 2000 - i.len() as i64, None)).collect(), ..Default::default() }
}

fn key_of(id: &str) -> String {
    post_key(&post(id, "carol", 0, None))
}

impl W {
    /// Wait until a feed has `n` new posts waiting.
    fn until_waiting(&mut self, feed: &FeedRef, n: usize) {
        let start = Instant::now();
        while self.app.social.waiting_count(&feed.account, &feed.feed) != n {
            self.frames(1);
            assert!(start.elapsed() < Duration::from_secs(10), "no new posts came");
            std::thread::sleep(Duration::from_millis(5));
        }
        self.frames(2);
    }

    /// The post at the top of the window: its key and the screen y of its top edge.
    fn top_post(&self) -> (String, f32) {
        let id = self.app.tabbar.tabs[self.app.tabbar.active].id;
        let k = self.app.social.anchors[&id].key.clone();
        let y = self.post_y(&k);
        (k, y)
    }

    fn post_y(&self, key: &str) -> f32 {
        self.app.social.post_rects.iter().find(|(k, _)| k == key).unwrap_or_else(|| panic!("{key} is not in the list")).1.min.y
    }

    fn posts(&self, feed: &FeedRef) -> Vec<String> {
        self.app.social.timelines[&(feed.account.clone(), feed.feed.clone())].posts.iter().map(|p| p.id.clone()).collect()
    }
}

// spec: SOC-027, SOC-028, SOC-023, SOC-029
#[test]
fn new_posts_wait_behind_a_button_and_showing_them_keeps_the_view_still() {
    let mut w = W::new("newposts");
    w.scroll(900.0);
    let (anchor, y0) = w.top_post();
    assert!(anchor.starts_with('h'), "{anchor}");
    // "Check now" finds two posts; the list does not change, nor does the view
    w.script.newer.lock().unwrap().push(("home".into(), fresh(&["n2", "n1"])));
    w.click("check-now");
    w.until_waiting(&home(), 2);
    assert!(w.rects_has("new-posts"));
    assert_eq!(w.title(0), "Mastodon · Home (2)");
    assert_eq!(w.posts(&home()).len(), 40);
    assert!((w.post_y(&anchor) - y0).abs() < 1.0, "the list did not move");
    // a second check adds to the waiting posts
    w.script.newer.lock().unwrap().push(("home".into(), fresh(&["n3"])));
    w.click("check-now");
    w.until_waiting(&home(), 3);
    assert!((w.post_y(&anchor) - y0).abs() < 1.0);
    // clicking the button: the posts go above, the top post of the window stays where it is,
    // frame by frame
    let p = w.rect("new-posts").center();
    w.frame(vec![Event::PointerMoved(p)]);
    let b = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    w.frame(vec![b(true)]);
    w.frame(vec![b(false)]);
    for i in 0..30 {
        assert!((w.post_y(&anchor) - y0).abs() < 1.0, "frame {i}: {} vs {y0}", w.post_y(&anchor));
        w.frames(1);
    }
    let shown = w.posts(&home());
    assert_eq!(&shown[..4], ["n3", "n2", "n1", "h0"], "the new posts are above, newest first");
    assert!(w.post_y(&key_of("n1")) < y0 - 50.0, "above the window, not in it");
    assert_eq!(w.title(0), "Mastodon · Home");
    assert!(!w.rects_has("new-posts"), "the button is gone");
    // scrolling up now moves the posts as the hand moves; nothing else moves
    w.scroll(-120.0);
    assert!((w.post_y(&anchor) - (y0 + 120.0)).abs() < 2.0, "{} vs {}", w.post_y(&anchor), y0 + 120.0);
    let (anchor2, y2) = w.top_post();
    w.frames(10);
    assert!((w.post_y(&anchor2) - y2).abs() < 1.0, "it stays there");
}

// spec: SOC-027
#[test]
fn the_button_sits_in_a_row_of_its_own_and_the_list_does_not_move_when_it_comes_and_goes() {
    let mut w = W::new("row");
    // at the top: the first post is the one the old button used to cover
    let list0 = w.app.social.list_rect;
    let (first, y0) = w.top_post();
    assert!(w.rects_has("up-to-date") && !w.rects_has("new-posts"), "the row says it is up to date");
    w.script.newer.lock().unwrap().push(("home".into(), fresh(&["n2", "n1"])));
    w.click("check-now");
    w.until_waiting(&home(), 2);
    let button = w.rect("new-posts");
    assert!(!w.rects_has("up-to-date"));
    let same = |w: &W, what: &str| {
        let l = w.app.social.list_rect;
        assert!((l.min.y - list0.min.y).abs() < 0.5 && (l.height() - list0.height()).abs() < 0.5, "{what}: list {l:?} vs {list0:?}");
        assert!((w.post_y(&first) - y0).abs() < 0.5, "{what}: post at {} vs {y0}", w.post_y(&first));
    };
    same(&w, "while the button is shown");
    // the button is above the list and covers no post
    assert!(button.max.y <= w.app.social.list_rect.min.y + 0.5, "{button:?} vs {:?}", w.app.social.list_rect);
    for (k, r) in &w.app.social.post_rects {
        assert!(!r.intersects(button), "the button covers {k}: {r:?} vs {button:?}");
    }
    w.click("new-posts");
    w.frames(10);
    assert!(!w.rects_has("new-posts"));
    same(&w, "after the click");
}

// spec: SOC-028
#[test]
fn the_view_stays_still_when_the_height_of_the_new_posts_changes_later() {
    let mut w = W::new("heights");
    w.scroll(500.0);
    let (anchor, y0) = w.top_post();
    // a new post with a content warning: its height changes when it is opened (as a picture's does when it arrives)
    let mut cw = post("cw", "carol", 5, None);
    cw.content_warning = Some("long".into());
    cw.text_markdown = "a long text. ".repeat(80);
    w.script.newer.lock().unwrap().push(("home".into(), Fresh { posts: vec![cw.clone(), post("n1", "carol", 4, None)], ..Default::default() }));
    w.click("check-now");
    w.until_waiting(&home(), 2);
    w.click("new-posts");
    assert!((w.post_y(&anchor) - y0).abs() < 1.0);
    for open in [true, false, true] {
        if open {
            w.app.social.revealed.insert(post_key(&cw));
        } else {
            w.app.social.revealed.clear();
        }
        for i in 0..5 {
            w.frames(1);
            assert!((w.post_y(&anchor) - y0).abs() < 1.0, "open={open} frame {i}: {} vs {y0}", w.post_y(&anchor));
        }
    }
}

// spec: SOC-029
#[test]
fn check_now_never_jumps_and_reload_from_the_top_does_replace_the_list() {
    let mut w = W::new("reload");
    w.scroll(700.0);
    let (anchor, y0) = w.top_post();
    w.click("check-now");
    // the check has come back
    w.wait_for("the check did not come back", |w| {
        let t = &w.app.social.timelines[&(home().account, home().feed)];
        t.last_checked.is_some() && !t.checking
    });
    w.frames(10);
    assert_eq!(w.posts(&home()).len(), 40);
    assert!((w.post_y(&anchor) - y0).abs() < 1.0, "no jump to the top");
    assert!(!w.rects_has("new-posts"), "nothing new: no button");
    // the menu next to it
    *w.script.first_page.lock().unwrap() = Some(Page { posts: feed_posts("r", 12), cursor: None, top: None });
    w.click("feed-menu");
    w.click("reload");
    let start = Instant::now();
    while w.posts(&home()).first().map(|p| p.as_str()) != Some("r0") {
        w.frames(1);
        assert!(start.elapsed() < Duration::from_secs(10), "the first page did not come");
        std::thread::sleep(Duration::from_millis(5));
    }
    w.frames(5);
    assert_eq!(w.posts(&home()).len(), 12);
    let top = w.post_y(&key_of_h("r0"));
    assert!(top > 0.0 && top < 200.0, "back at the top: {top}");
}

// spec: SOC-041
#[test]
fn a_running_check_shows_its_spinner_in_the_check_now_button_and_nothing_moves() {
    let mut w = W::new("spinner");
    w.frames(60);
    let (menu, button) = (w.rect("feed-menu"), w.rect("check-now"));
    assert!(!w.rects_has("check-now-spinner"));
    let key = (home().account, home().feed);
    w.app.social.timelines.get_mut(&key).unwrap().checking = true;
    w.frames(5);
    assert!(w.rects_has("check-now-spinner"), "the spinner shows");
    assert!(button.contains_rect(w.rect("check-now-spinner")), "inside the button");
    assert_eq!(w.rect("feed-menu"), menu, "the menu button stays put");
    assert_eq!(w.rect("check-now"), button, "the button keeps its size");
    w.app.social.timelines.get_mut(&key).unwrap().checking = false;
    w.frames(5);
    assert!(!w.rects_has("check-now-spinner"));
    assert_eq!(w.rect("feed-menu"), menu);
}

#[test]
// spec: SOC-042
fn saving_and_filling_a_gap_show_their_spinner_in_the_button_and_nothing_moves() {
    let mut w = W::new("spinner-save");
    w.frames(60);
    let key = (home().account, home().feed);
    let first = post_key(&w.app.social.timelines[&key].posts[0]);
    w.app.social.timelines.get_mut(&key).unwrap().gaps.push(super::state::Gap { above: first.clone(), cursor: "older".into() });
    w.frames(5);
    let (save, open, copy, gap) = (w.rect("save:0"), w.rect("open:0"), w.rect("copy:0"), w.rect("gap:0"));
    assert!(!w.rects_has("save-spinner:0") && !w.rects_has("gap-spinner:0"));
    w.app.social.saving.insert(first);
    w.app.social.timelines.get_mut(&key).unwrap().filling = true;
    w.frames(5);
    assert!(save.contains_rect(w.rect("save-spinner:0")), "the save spinner is inside its button");
    assert!(gap.contains_rect(w.rect("gap-spinner:0")), "the gap spinner is inside its button");
    assert_eq!((w.rect("save:0"), w.rect("open:0"), w.rect("copy:0"), w.rect("gap:0")), (save, open, copy, gap), "nothing moves or changes size");
    w.app.social.saving.clear();
    w.app.social.timelines.get_mut(&key).unwrap().filling = false;
    w.frames(5);
    assert!(!w.rects_has("save-spinner:0") && !w.rects_has("gap-spinner:0"));
    assert_eq!((w.rect("save:0"), w.rect("open:0"), w.rect("copy:0"), w.rect("gap:0")), (save, open, copy, gap));
}

fn key_of_h(id: &str) -> String {
    post_key(&post(id, "bob", 0, None))
}

impl W {
    fn rects_has(&self, key: &str) -> bool {
        self.app.social.rects.iter().any(|r| r.0 == key)
    }
}

// spec: SOC-028
#[test]
fn the_period_key_shows_the_new_posts_unless_a_dialog_is_open() {
    let mut w = W::new("key");
    w.script.newer.lock().unwrap().push(("home".into(), fresh(&["n1"])));
    w.click("check-now");
    w.until_waiting(&home(), 1);
    let key = |pressed| Event::Key { key: egui::Key::Period, physical_key: Some(egui::Key::Period), pressed, repeat: false, modifiers: Modifiers::NONE };
    w.app.modal = Some(crate::app::Modal::Social(Box::default()));
    w.frames(3);
    w.frame(vec![key(true)]);
    w.frame(vec![key(false)]);
    w.frames(3);
    assert_eq!(w.app.social.waiting_count(&home().account, "home"), 1, "a dialog has the keys");
    w.app.modal = None;
    w.frames(3);
    w.frame(vec![key(true)]);
    w.frame(vec![key(false)]);
    w.frames(3);
    assert_eq!(w.posts(&home())[0], "n1");
    assert_eq!(w.app.social.waiting_count(&home().account, "home"), 0);
}

// spec: SOC-025, SOC-023
#[test]
fn a_feed_in_a_background_tab_is_checked_on_time_and_its_tab_title_shows_the_count() {
    let mut w = W::new("background");
    w.click("feed-combo");
    w.click("feed-new-tab:list:1");
    assert_eq!(w.app.tabbar.active, 1);
    // the first tab (Home) is in the background now: its timer comes due
    w.script.newer.lock().unwrap().push(("home".into(), fresh(&["n1", "n2", "n3"])));
    w.script.newer.lock().unwrap().push(("list:1".into(), fresh(&["x1"])));
    for f in [home(), list()] {
        w.app.social.timelines.get_mut(&(f.account, f.feed)).unwrap().next_check = Some(Instant::now());
    }
    w.until_waiting(&home(), 3);
    w.until_waiting(&list(), 1);
    assert_eq!((w.title(0), w.title(1)), ("Mastodon · Home (3)".to_string(), "Mastodon · List: Rust people (1)".to_string()));
    // switched off in the settings: nothing is asked for any more
    w.app.cfg.social_check_minutes = 0;
    w.script.newer.lock().unwrap().push(("home".into(), fresh(&["n9"])));
    w.app.social.timelines.get_mut(&(home().account, home().feed)).unwrap().next_check = Some(Instant::now());
    w.frames(10);
    std::thread::sleep(Duration::from_millis(50));
    w.frames(5);
    assert_eq!(w.app.social.waiting_count(&home().account, "home"), 3);
    // the setting is made in the accounts dialog
    w.app.social_dialog_action(crate::social::accounts_ui::DialogAction::CheckMinutes(7));
    assert_eq!(w.app.cfg.social_check_minutes, 7);
}

// spec: SOC-028
#[test]
fn without_a_second_pass_the_view_is_put_back_in_the_next_frame() {
    let mut w = W::new("onepass");
    w.ctx.options_mut(|o| o.max_passes = 1.try_into().unwrap());
    w.scroll(900.0);
    let (anchor, y0) = w.top_post();
    w.script.newer.lock().unwrap().push(("home".into(), fresh(&["n2", "n1"])));
    w.click("check-now");
    w.until_waiting(&home(), 2);
    w.click("new-posts");
    w.frames(3);
    assert!((w.post_y(&anchor) - y0).abs() < 1.0, "{} vs {y0}", w.post_y(&anchor));
}

impl W {
    /// Run frames until `until` holds (jobs finish in the background).
    fn wait_for(&mut self, what: &str, until: impl Fn(&W) -> bool) {
        let start = Instant::now();
        while !until(self) {
            self.frames(1);
            assert!(start.elapsed() < Duration::from_secs(10), "{what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn hit_of(account: &str, feed: &str, post: Post) -> crate::social::post_index::PostHit {
    crate::social::post_index::PostHit {
        account: account.into(),
        feed: feed.into(),
        key: post_key(&post),
        author_name: String::new(),
        author_handle: String::new(),
        created_at: 0,
        snippet: String::new(),
        url: String::new(),
        reply_to: None,
        post,
    }
}

// spec: SOC-037
#[test]
fn saving_a_thread_puts_its_posts_and_replies_into_the_index() {
    let mut w = W::new("saveindex");
    let entry = demo::account();
    let acct = entry.account.id.clone();
    assert_eq!(w.app.social.index.count(None), 0);
    let ctx = w.ctx.clone();
    w.app.social.save(&ctx, &entry, "home", &demo::posts().remove(4));
    w.wait_for("the save did not finish", |w| w.app.social.index.count(None) > 0);
    // the three posts of the thread and the three replies of other people
    assert_eq!(w.app.social.index.count(Some(&acct)), 6);
    let hits = w.app.social.index.search("grep", 10);
    assert_eq!((hits.len(), hits[0].post.id.as_str(), hits[0].feed.as_str()), (1, "demo-reply-3", "home"), "a reply of another person");
    // a reply has the text of the thread post it answers as context
    let ids = |q: &str| w.app.social.index.search(q, 10).iter().map(|h| h.post.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids("in:\"folder I mean\""), ["demo-reply-3"]);
    assert_eq!(ids("in:\"Formats with one reader\""), ["demo-reply-1", "demo-thread-3"], "the two replies to the second post");
    assert_eq!(ids("folder").len(), 2, "the third post of the thread, and the reply that has it as context");
}

// spec: SOC-039
#[test]
fn a_hit_whose_post_is_gone_is_fetched_from_the_network_and_an_error_is_shown() {
    let mut w = W::new("fetchhit");
    let (acct, list_key) = (home().account, (home().account, "list:1".to_string()));
    let mut gone = post("gone1", "bob", crate::calendar::time::now() - 3600 * 11 - 1800, None);
    gone.text_markdown = "an unusual walrus".into();
    // a hit of the search, then the index is cleared (Settings) before the click: the index is empty and the feed has no such post
    w.app.social.index.add(&acct, "list:1", std::slice::from_ref(&gone));
    let hit = w.app.social.index.search("walrus", 5).remove(0);
    w.app.social.index.clear();
    *w.script.posts.lock().unwrap() = vec![Post { text_markdown: "an unusual walrus, from the server".into(), ..gone.clone() }];
    w.app.open_post_hit(&hit);
    assert_eq!((w.app.tabbar.tabs.len(), w.app.tabbar.active), (2, 1), "the feed opens in a new tab at once");
    w.wait_for("the post was not fetched", |w| w.app.social.timelines[&list_key].posts.iter().any(|p| p.id == "gone1"));
    w.frames(4);
    assert_eq!(*w.script.post_calls.lock().unwrap(), ["gone1"]);
    let t = &w.app.social.timelines[&list_key];
    assert_eq!(t.posts.iter().find(|p| p.id == "gone1").unwrap().text_markdown, "an unusual walrus, from the server");
    assert!(w.app.social.index.contains(&acct, &post_key(&gone)), "indexed again");
    let y = w.app.social.post_rects.iter().find(|r| r.0 == post_key(&gone)).expect("shown").1.min.y - w.app.social.list_rect.min.y;
    assert!(y.abs() < 2.0, "at the top of the list: {y}");
    // a post the server does not know: a message, and the list is as it was
    let n = w.app.social.timelines[&list_key].posts.len();
    w.app.social.index.clear();
    w.app.open_post_hit(&hit_of(&acct, "list:1", Post { id: "nowhere".into(), ..gone }));
    w.wait_for("no message", |w| w.app.toasts.iter().any(|t| t.0.contains("Could not fetch the post")));
    assert_eq!(w.app.social.timelines[&list_key].posts.len(), n);
    assert!(w.app.social.focus.is_none());
    // a post the feed still holds is not fetched
    let held = w.app.social.timelines[&list_key].posts[5].clone();
    let calls = w.script.post_calls.lock().unwrap().len();
    w.app.open_post_hit(&hit_of(&acct, "list:1", held));
    w.frames(6);
    assert_eq!(w.script.post_calls.lock().unwrap().len(), calls);
}

// spec: SOC-040
#[test]
fn the_settings_clear_and_rebuild_the_post_index() {
    let mut w = W::new("indexsettings");
    let acct = home().account;
    // what the feeds hold is not indexed (the fixtures were put in without a load); something older is
    for (f, p) in [("home", "h"), ("list:1", "l")] {
        w.app.social.timelines.get_mut(&(acct.clone(), f.to_string())).unwrap().posts = recent(p, 40);
    }
    let old = post("old1", "bob", crate::calendar::time::now() - 3600, None);
    w.app.social.index.add(&acct, "home", std::slice::from_ref(&old));
    w.app.open_settings(crate::app::SettingsTab::Social);
    w.frames(3);
    assert_eq!(w.app.social.index.count(None), 1);
    // Rebuild: clears and indexes what the open feeds hold (two lists of 40; a waiting post too)
    w.app.social.timelines.get_mut(&(acct.clone(), "home".into())).unwrap().waiting = vec![post("waiting1", "bob", crate::calendar::time::now() - 5, None)];
    // the dialog scrolls: bring the section into view (with room below for the confirmation)
    while w.rect("index-rebuild").max.y > 780.0 {
        w.scroll(150.0);
    }
    w.click("index-rebuild");
    assert_eq!(w.app.social.index.count(None), 81);
    assert!(w.app.social.index.search("old1", 5).is_empty(), "what no feed holds is gone");
    assert!(w.app.toasts.iter().any(|t| t.0.contains("rebuilt: 81")));
    // Clear asks first
    w.click("index-clear");
    assert!(w.app.social.confirm_clear);
    assert_eq!(w.app.social.index.count(None), 81, "nothing is deleted before the answer");
    w.click("index-clear-no");
    assert!(!w.app.social.confirm_clear);
    assert_eq!(w.app.social.index.count(None), 81);
    w.click("index-clear");
    w.click("index-clear-yes");
    assert!(!w.app.social.confirm_clear);
    assert_eq!(w.app.social.index.count(None), 0);
    assert_eq!(w.app.social.timelines[&(acct, "home".to_string())].posts.len(), 40, "the feeds are untouched");
}
