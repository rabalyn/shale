//! What the feed view remembers: the chosen account and feed, the loaded posts of every feed,
//! and the background jobs (feeds, pages, saving a post). Jobs send their result over a channel
//! that `poll` reads each frame; a job that panics sends an error instead, so no spinner runs
//! forever (ADR-0006).

use super::images::ImageCache;
use super::note::SavedPost;
use super::{Client, Feed, FeedRef, Fresh, Page, Post, Since, SocialAccountEntry};
use eframe::egui::{Context, Rect};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How many posts may wait to be shown; then the checks pause (SOC-026).
pub const MAX_WAITING: usize = 1000;

/// Where the posts come from when it is not a real account: screenshots (`demo`) and tests plug
/// in a pretend one. The jobs ask it instead of a `Client`.
pub trait Source: Send + Sync {
    fn page(&self, feed: &Feed, cursor: Option<&str>) -> Result<Page, String>;
    fn newer(&self, feed: &Feed, since: &Since) -> Result<Fresh, String>;
}

/// A hole in a list: posts between the post `above` (a `post_key`) and the one below it were not
/// fetched (Bluesky: a check hit its limit). `cursor` fetches the page that goes on from there.
#[derive(Clone, Debug, PartialEq)]
pub struct Gap {
    pub above: String,
    pub cursor: String,
}

/// How long to wait after `failures` failed checks in a row: twice as long each time, up to 16 times.
pub fn backoff(interval: Duration, failures: u32) -> Duration {
    interval * (1u32 << failures.min(4))
}

pub enum FeedsState {
    Loading,
    Ready(Vec<Feed>),
    Failed(String),
}

/// The posts of one feed (of one account) loaded so far.
#[derive(Default)]
pub struct Timeline {
    pub posts: Vec<Post>,
    /// for the next older page; None at the end
    pub cursor: Option<String>,
    pub loading: bool,
    /// the first page has arrived
    pub loaded: bool,
    pub error: Option<String>,
    /// Mastodon: the id of the newest timeline entry loaded or waiting (`Page::top`)
    pub top: Option<String>,
    /// posts found by checks, newest first; not in `posts` until the user asks (SOC-027)
    pub waiting: Vec<Post>,
    pub gaps: Vec<Gap>,
    /// a check for newer posts runs
    pub checking: bool,
    /// a gap is being filled
    pub filling: bool,
    /// the last check failed (the list stays as it is)
    pub check_error: Option<String>,
    /// checks that failed in a row
    pub failures: u32,
    /// when the next check is due; None: not planned yet
    pub next_check: Option<Instant>,
    /// how often the list was replaced by a first page (a view that sees a new number starts at the top)
    pub reloads: u32,
}

impl Timeline {
    /// Take what a check found: new posts wait (no doubles; a post that waits already follows the
    /// newest data), and the limit of a Bluesky check becomes a gap below its last post.
    fn absorb(&mut self, fresh: Fresh) {
        let in_list: HashSet<String> = self.posts.iter().map(post_key).collect();
        let mut fresh_new: Vec<Post> = vec![];
        let mut seen: HashSet<String> = HashSet::new();
        for p in fresh.posts {
            let k = post_key(&p);
            if in_list.contains(&k) || !seen.insert(k.clone()) {
                continue;
            }
            match self.waiting.iter_mut().find(|w| post_key(w) == k) {
                Some(w) => *w = p,
                None => fresh_new.push(p),
            }
        }
        if let (Some(cursor), Some(last)) = (fresh.gap, fresh_new.last()) {
            self.gaps.push(Gap { above: post_key(last), cursor });
        }
        fresh_new.append(&mut self.waiting);
        self.waiting = fresh_new;
        if fresh.top.is_some() {
            self.top = fresh.top;
        }
    }

    /// A page for the gap below `above`: the posts up to the first one already known go in
    /// there. The gap is closed when the page met a known post (or the feed ended), else it moves
    /// below the last post that came in.
    fn fill(&mut self, above: &str, page: Page) {
        let Some(at) = self.posts.iter().position(|p| post_key(p) == above) else {
            self.gaps.retain(|g| g.above != above);
            return;
        };
        let known: HashSet<String> = self.posts.iter().chain(&self.waiting).map(post_key).collect();
        let mut new: Vec<Post> = vec![];
        let mut met = false;
        for p in page.posts {
            if known.contains(&post_key(&p)) {
                met = true;
                break;
            }
            new.push(p);
        }
        self.gaps.retain(|g| g.above != above);
        if !met && !new.is_empty() {
            if let Some(cursor) = page.cursor {
                self.gaps.push(Gap { above: post_key(new.last().expect("not empty")), cursor });
            }
        }
        self.posts.splice(at + 1..at + 1, new);
    }
}

enum Msg {
    Feeds { account: String, client: Option<Client>, result: Result<Vec<Feed>, String> },
    Page { key: (String, String), client: Option<Client>, replace: bool, result: Result<Page, String> },
    Check { key: (String, String), client: Option<Client>, result: Result<Fresh, String> },
    Gap { key: (String, String), above: String, client: Option<Client>, result: Result<Page, String> },
    Saved { post: String, label: &'static str, result: Result<SavedPost, String> },
}

pub struct SocialState {
    /// the feed shown last for each account (feed ids): what a tab shows that names no feed
    pub feed_of: HashMap<String, String>,
    pub feeds: HashMap<String, FeedsState>,
    pub timelines: HashMap<(String, String), Timeline>,
    /// posts being fetched for saving (see `post_key`)
    pub saving: HashSet<String>,
    /// posts whose content warning was opened
    pub revealed: HashSet<String>,
    /// the posts are made-up ones (screenshots, tests): nothing goes to the network
    pub demo: bool,
    pub images: Option<ImageCache>,
    pub cache_dir: PathBuf,
    /// finished saves waiting for the Import dialog: the post, thread and media, and the network
    pub saved: Vec<(SavedPost, &'static str)>,
    /// messages for the user (failed saves)
    pub messages: Vec<String>,
    /// where the controls are (for tests)
    pub rects: Vec<(String, Rect)>,
    /// where the posts of the list shown are, by `post_key` (for tests)
    pub post_rects: Vec<(String, Rect)>,
    /// how often the feeds open in tabs are checked for new posts (zero: never)
    pub check_every: Duration,
    /// the posts of a pretend server (screenshots, tests); None: the real account
    pub source: Option<Arc<dyn Source>>,
    /// where each tab's view is held still while posts come in above it (see `feed_ui`)
    pub anchors: HashMap<u64, super::feed_ui::Anchor>,
    clients: HashMap<String, Client>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

/// Identifies a post in the feed: the same post boosted by two people shows twice.
pub fn post_key(p: &Post) -> String {
    format!("{}|{}", p.id, p.reposted_by.as_ref().map(|b| b.id.as_str()).unwrap_or(""))
}

fn run(ctx: &Context, tx: &Sender<Msg>, job: impl FnOnce() -> Msg + Send + 'static, fail: impl FnOnce(String) -> Msg + Send + 'static) {
    let (ctx, tx) = (ctx.clone(), tx.clone());
    crate::jobs::run(job, move |r| {
        let msg = r.unwrap_or_else(fail);
        let _ = tx.send(msg);
        ctx.request_repaint();
    });
}

impl SocialState {
    pub fn new(cache_dir: PathBuf) -> SocialState {
        let (tx, rx) = channel();
        SocialState {
            feed_of: HashMap::new(),
            feeds: HashMap::new(),
            timelines: HashMap::new(),
            saving: HashSet::new(),
            revealed: HashSet::new(),
            demo: false,
            images: None,
            cache_dir,
            saved: vec![],
            messages: vec![],
            rects: vec![],
            post_rects: vec![],
            check_every: Duration::from_secs(300),
            source: None,
            anchors: HashMap::new(),
            clients: HashMap::new(),
            tx,
            rx,
        }
    }

    /// The account a tab shows: its own if it still exists, else the one used last, else the first.
    pub fn account_for<'a>(accounts: &'a [SocialAccountEntry], want: Option<&str>, last: Option<&(String, String)>) -> Option<&'a SocialAccountEntry> {
        let find = |id: Option<&str>| id.and_then(|id| accounts.iter().find(|e| e.account.id == id));
        find(want).or_else(|| find(last.map(|l| l.0.as_str()))).or(accounts.first())
    }

    /// The feed a tab shows for the account: the one it names, else the feed shown last for the
    /// account, else the first. None until the feed list is known.
    pub fn resolve_feed(&self, account: &str, want: &str, last: Option<&(String, String)>) -> Option<Feed> {
        let FeedsState::Ready(list) = self.feeds.get(account)? else { return None };
        let by_id = |id: &str| list.iter().find(|f| f.id == id);
        let remembered = self.feed_of.get(account).map(String::as_str).or(last.filter(|l| l.0 == account).map(|l| l.1.as_str()));
        by_id(want).or_else(|| remembered.and_then(by_id)).or(list.first()).cloned()
    }

    /// Forget everything about an account (removed).
    pub fn forget(&mut self, account: &str) {
        self.clients.remove(account);
        self.feeds.remove(account);
        self.feed_of.remove(account);
        self.timelines.retain(|k, _| k.0 != account);
    }

    fn client_source(&self, e: &SocialAccountEntry) -> impl FnOnce() -> Client + Send + 'static {
        let (cached, entry) = (self.clients.get(&e.account.id).cloned(), e.clone());
        move || cached.unwrap_or_else(|| Client::new(entry.account.clone(), &entry.secret()))
    }

    /// Ask for the feed list of the account (once; `reload_feeds` asks again).
    pub fn ensure_feeds(&mut self, ctx: &Context, e: &SocialAccountEntry) {
        if self.feeds.contains_key(&e.account.id) {
            return;
        }
        self.feeds.insert(e.account.id.clone(), FeedsState::Loading);
        let (client, id) = (self.client_source(e), e.account.id.clone());
        let id2 = id.clone();
        run(
            ctx,
            &self.tx,
            move || {
                let mut c = client();
                let result = c.feeds();
                Msg::Feeds { account: id, client: Some(c), result }
            },
            move |m| Msg::Feeds { account: id2, client: None, result: Err(m) },
        );
    }

    pub fn reload_feeds(&mut self, ctx: &Context, e: &SocialAccountEntry) {
        self.feeds.remove(&e.account.id);
        self.ensure_feeds(ctx, e);
    }

    /// Load a page of a feed: the first (`cursor` None; `replace` the posts when it arrives) or the next.
    fn load(&mut self, ctx: &Context, e: &SocialAccountEntry, feed: &Feed, cursor: Option<String>, replace: bool) {
        let key = (e.account.id.clone(), feed.id.clone());
        let t = self.timelines.entry(key.clone()).or_default();
        if t.loading {
            return;
        }
        t.loading = true;
        t.error = None;
        let (client, src, feed, key2) = (self.client_source(e), self.source.clone(), feed.clone(), key.clone());
        run(
            ctx,
            &self.tx,
            move || {
                let (client, result) = match src {
                    Some(s) => (None, s.page(&feed, cursor.as_deref())),
                    None => {
                        let mut c = client();
                        let r = c.timeline(&feed, cursor.as_deref());
                        (Some(c), r)
                    }
                };
                Msg::Page { key, client, replace, result }
            },
            move |m| Msg::Page { key: key2, client: None, replace, result: Err(m) },
        );
    }

    /// Start the first page of a feed that was never loaded.
    pub fn ensure_timeline(&mut self, ctx: &Context, e: &SocialAccountEntry, feed: &Feed) {
        if self.demo && self.source.is_none() {
            return;
        }
        let t = self.timelines.get(&(e.account.id.clone(), feed.id.clone()));
        if t.map(|t| !t.loaded && !t.loading && t.error.is_none()).unwrap_or(true) {
            self.load(ctx, e, feed, None, true);
        }
    }

    /// Load the next older page (the end of the list came into view).
    pub fn load_more(&mut self, ctx: &Context, e: &SocialAccountEntry, feed: &Feed) {
        let cursor = self.timelines.get(&(e.account.id.clone(), feed.id.clone())).and_then(|t| if t.loading || t.error.is_some() { None } else { t.cursor.clone() });
        if let Some(c) = cursor.filter(|_| !self.demo || self.source.is_some()) {
            self.load(ctx, e, feed, Some(c), false);
        }
    }

    /// Start again from the newest post: the list is replaced when the first page arrives (the
    /// waiting posts and gaps are dropped with it). "Check now" is `check`.
    pub fn reload(&mut self, ctx: &Context, e: &SocialAccountEntry, feed: &Feed) {
        if self.demo && self.source.is_none() {
            return;
        }
        if let Some(t) = self.timelines.get_mut(&(e.account.id.clone(), feed.id.clone())) {
            t.error = None;
        }
        self.load(ctx, e, feed, None, true);
    }

    /// Look for posts newer than the newest one known, in the background. They are not put into
    /// the list: they wait (`Timeline::waiting`). False when nothing was started (no list yet, a
    /// page or a check is running, too many posts wait).
    pub fn check(&mut self, ctx: &Context, e: &SocialAccountEntry, feed: &Feed) -> bool {
        if self.demo && self.source.is_none() {
            return false;
        }
        let key = (e.account.id.clone(), feed.id.clone());
        let Some(t) = self.timelines.get_mut(&key) else { return false };
        if !t.loaded || t.loading || t.checking || t.waiting.len() >= MAX_WAITING {
            return false;
        }
        t.checking = true;
        let since = Since { top: t.top.clone(), known: t.posts.iter().chain(&t.waiting).map(post_key).collect() };
        let (client, src, feed, key2) = (self.client_source(e), self.source.clone(), feed.clone(), key.clone());
        run(
            ctx,
            &self.tx,
            move || {
                let (client, result) = match src {
                    Some(s) => (None, s.newer(&feed, &since)),
                    None => {
                        let mut c = client();
                        let r = c.newer(&feed, &since);
                        (Some(c), r)
                    }
                };
                Msg::Check { key, client, result }
            },
            move |m| Msg::Check { key: key2, client: None, result: Err(m) },
        );
        true
    }

    /// The timer: start a check for every feed open in a tab (`open`; background tabs too) whose
    /// time has come. A feed open in two tabs is checked once.
    pub fn tick(&mut self, ctx: &Context, accounts: &[SocialAccountEntry], open: &[FeedRef], now: Instant) {
        if self.check_every.is_zero() {
            return;
        }
        let mut seen = HashSet::new();
        for f in open.iter().filter(|f| !f.feed.is_empty() && seen.insert((*f).clone())) {
            let Some(e) = accounts.iter().find(|e| e.account.id == f.account) else { continue };
            let Some(FeedsState::Ready(list)) = self.feeds.get(&f.account) else { continue };
            let Some(feed) = list.iter().find(|x| x.id == f.feed).cloned() else { continue };
            let Some(t) = self.timelines.get_mut(&(f.account.clone(), f.feed.clone())) else { continue };
            match t.next_check {
                None => t.next_check = Some(now + self.check_every),
                Some(at) if now >= at => {
                    // a check that cannot start now (a page is loading) is tried again next frame
                    self.check(ctx, e, &feed);
                }
                _ => {}
            }
        }
    }

    /// Put the waiting posts of a feed above its list. The number of posts put in.
    pub fn show_waiting(&mut self, key: &(String, String)) -> usize {
        let Some(t) = self.timelines.get_mut(key) else { return 0 };
        let have: HashSet<String> = t.posts.iter().map(post_key).collect();
        let fresh: Vec<Post> = std::mem::take(&mut t.waiting).into_iter().filter(|p| !have.contains(&post_key(p))).collect();
        let n = fresh.len();
        t.posts.splice(0..0, fresh);
        n
    }

    /// How many new posts wait for a feed.
    pub fn waiting_count(&self, account: &str, feed: &str) -> usize {
        self.timelines.get(&(account.to_string(), feed.to_string())).map(|t| t.waiting.len()).unwrap_or(0)
    }

    /// Fetch the page that fills the gap below the post `above`.
    pub fn fill_gap(&mut self, ctx: &Context, e: &SocialAccountEntry, feed: &Feed, above: &str) {
        let key = (e.account.id.clone(), feed.id.clone());
        let Some(t) = self.timelines.get_mut(&key) else { return };
        let Some(gap) = t.gaps.iter().find(|g| g.above == above).cloned() else { return };
        if t.filling {
            return;
        }
        t.filling = true;
        let (client, src, feed, key2, above2) = (self.client_source(e), self.source.clone(), feed.clone(), key.clone(), above.to_string());
        run(
            ctx,
            &self.tx,
            move || {
                let (client, result) = match src {
                    Some(s) => (None, s.page(&feed, Some(&gap.cursor))),
                    None => {
                        let mut c = client();
                        let r = c.timeline(&feed, Some(&gap.cursor));
                        (Some(c), r)
                    }
                };
                Msg::Gap { key, above: gap.above, client, result }
            },
            move |m| Msg::Gap { key: key2, above: above2, client: None, result: Err(m) },
        );
    }

    /// Fetch the thread and the media of a post for the Import dialog.
    pub fn save(&mut self, ctx: &Context, e: &SocialAccountEntry, post: &Post) {
        let key = post_key(post);
        if !self.saving.insert(key.clone()) {
            return;
        }
        let (client, post, label, demo, key2) = (self.client_source(e), post.clone(), e.account.network.label(), self.demo, key.clone());
        run(
            ctx,
            &self.tx,
            move || {
                let result = if demo { Ok(super::demo::saved(&post)) } else { Ok(client().save(&post)) };
                Msg::Saved { post: key, label, result }
            },
            move |m| Msg::Saved { post: key2, label, result: Err(m) },
        );
    }

    /// Take the results of finished jobs. True if anything arrived.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok(m) = self.rx.try_recv() {
            any = true;
            match m {
                Msg::Feeds { account, client, result } => {
                    if let Some(c) = client {
                        self.clients.insert(account.clone(), c);
                    }
                    self.feeds.insert(account, result.map(FeedsState::Ready).unwrap_or_else(FeedsState::Failed));
                }
                Msg::Page { key, client, replace, result } => {
                    if let Some(c) = client {
                        self.clients.insert(key.0.clone(), c);
                    }
                    let t = self.timelines.entry(key).or_default();
                    t.loading = false;
                    match result {
                        Ok(page) => {
                            if replace {
                                t.posts = page.posts;
                                t.cursor = page.cursor;
                                // a fresh start: what waited or was missing is part of it
                                t.reloads += 1;
                                t.top = page.top;
                                t.waiting.clear();
                                t.gaps.clear();
                                t.failures = 0;
                                t.check_error = None;
                                t.next_check = Some(Instant::now() + self.check_every);
                            } else {
                                let have: HashSet<String> = t.posts.iter().map(post_key).collect();
                                let fresh: Vec<Post> = page.posts.into_iter().filter(|p| !have.contains(&post_key(p))).collect();
                                // a page with nothing new would be asked for again and again
                                t.cursor = if fresh.is_empty() { None } else { page.cursor };
                                t.posts.extend(fresh);
                            }
                            t.loaded = true;
                        }
                        Err(e) => t.error = Some(e),
                    }
                }
                Msg::Check { key, client, result } => {
                    if let Some(c) = client {
                        self.clients.insert(key.0.clone(), c);
                    }
                    let every = self.check_every;
                    let t = self.timelines.entry(key).or_default();
                    t.checking = false;
                    match result {
                        Ok(fresh) => {
                            t.failures = 0;
                            t.check_error = None;
                            t.next_check = Some(Instant::now() + every);
                            t.absorb(fresh);
                        }
                        Err(e) => {
                            t.failures += 1;
                            t.check_error = Some(e);
                            t.next_check = Some(Instant::now() + backoff(every, t.failures));
                        }
                    }
                }
                Msg::Gap { key, above, client, result } => {
                    if let Some(c) = client {
                        self.clients.insert(key.0.clone(), c);
                    }
                    let t = self.timelines.entry(key).or_default();
                    t.filling = false;
                    match result {
                        Ok(page) => t.fill(&above, page),
                        Err(e) => t.check_error = Some(e),
                    }
                }
                Msg::Saved { post, label, result } => {
                    self.saving.remove(&post);
                    match result {
                        Ok(s) => self.saved.push((s, label)),
                        Err(e) => self.messages.push(format!("Could not save the post: {e}")),
                    }
                }
            }
        }
        any
    }

    /// Fill the view with made-up posts (screenshots, tests).
    pub fn load_demo(&mut self, account: &SocialAccountEntry) {
        self.demo = true;
        let id = account.account.id.clone();
        self.feeds.insert(id.clone(), FeedsState::Ready(super::demo::feeds()));
        self.feed_of.insert(id.clone(), "home".into());
        // the first check comes at once, so the screenshot shows the button for new posts
        self.source = Some(Arc::new(super::demo::DemoSource));
        self.timelines.insert((id, "home".into()), Timeline { posts: super::demo::posts(), loaded: true, next_check: Some(Instant::now()), ..Default::default() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::social::tests::post;

    fn wait(st: &mut SocialState, until: impl Fn(&SocialState) -> bool) {
        for _ in 0..200 {
            st.poll();
            if until(st) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the job did not finish");
    }

    // spec: SOC-010
    #[test]
    fn a_crashing_job_ends_with_an_error() {
        let ctx = Context::default();
        let mut st = SocialState::new(std::env::temp_dir());
        // a job that panics
        let tx = st.tx.clone();
        run(&ctx, &tx, || panic!("boom"), |m| Msg::Saved { post: "p".into(), label: "Mastodon", result: Err(m) });
        st.saving.insert("p".into());
        wait(&mut st, |s| s.saving.is_empty());
        assert!(st.messages[0].contains("crashed"), "{:?}", st.messages);
    }

    // spec: SOC-009
    #[test]
    fn pages_are_added_without_doubles_and_the_end_is_noticed() {
        let mut st = SocialState::new(std::env::temp_dir());
        let key = ("a".to_string(), "home".to_string());
        let page = |ids: &[&str], cursor: Option<&str>| Page { posts: ids.iter().map(|i| post(i, "bob", 1, None)).collect(), cursor: cursor.map(String::from), top: None };
        let send = |st: &SocialState, replace, result| st.tx.send(Msg::Page { key: key.clone(), client: None, replace, result }).unwrap();
        send(&st, true, Ok(page(&["1", "2"], Some("c2"))));
        send(&st, false, Ok(page(&["2", "3"], Some("c3"))));
        st.poll();
        let t = &st.timelines[&key];
        assert_eq!(t.posts.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["1", "2", "3"]);
        assert_eq!(t.cursor.as_deref(), Some("c3"));
        send(&st, false, Ok(page(&["3"], Some("c4"))));
        st.poll();
        assert_eq!(st.timelines[&key].cursor, None, "nothing new: stop asking");
        send(&st, true, Err("offline".into()));
        st.poll();
        assert_eq!(st.timelines[&key].error.as_deref(), Some("offline"));
        assert_eq!(st.timelines[&key].posts.len(), 3, "an error keeps what is shown");
    }

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// A pretend server: the answers to checks and pages are lined up, every call is counted, and
    /// a check can be held until the test lets it go.
    #[derive(Default)]
    struct Fake {
        newer: Mutex<Vec<Result<Fresh, String>>>,
        pages: Mutex<HashMap<String, Page>>,
        newer_calls: Mutex<Vec<(String, Since)>>,
        page_calls: AtomicUsize,
        gate: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    }

    impl Source for Fake {
        fn page(&self, _feed: &Feed, cursor: Option<&str>) -> Result<Page, String> {
            self.page_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.pages.lock().unwrap().get(cursor.unwrap_or("")).cloned().unwrap_or_default())
        }
        fn newer(&self, feed: &Feed, since: &Since) -> Result<Fresh, String> {
            self.newer_calls.lock().unwrap().push((feed.id.clone(), since.clone()));
            if let Some(rx) = self.gate.lock().unwrap().as_ref() {
                let _ = rx.recv();
            }
            let mut q = self.newer.lock().unwrap();
            if q.is_empty() {
                Ok(Fresh::default())
            } else {
                q.remove(0)
            }
        }
    }

    fn fresh(ids: &[&str]) -> Fresh {
        Fresh { posts: ids.iter().map(|i| post(i, "bob", 1, None)).collect(), ..Default::default() }
    }

    fn ids(posts: &[Post]) -> Vec<&str> {
        posts.iter().map(|p| p.id.as_str()).collect()
    }

    /// A state with the demo account, two feeds with a loaded list each, and a pretend server.
    fn ready() -> (SocialState, SocialAccountEntry, Arc<Fake>, Context) {
        let e = crate::social::demo::account();
        let mut st = SocialState::new(std::env::temp_dir());
        let fake = Arc::new(Fake::default());
        st.source = Some(fake.clone());
        st.feeds.insert(e.account.id.clone(), FeedsState::Ready(crate::social::demo::feeds()));
        for f in ["home", "list:1"] {
            st.timelines.insert((e.account.id.clone(), f.into()), Timeline { posts: ["k2", "k1"].iter().map(|i| post(&format!("{f}-{i}"), "bob", 1, None)).collect(), loaded: true, ..Default::default() });
        }
        (st, e, fake, Context::default())
    }

    fn home_key(e: &SocialAccountEntry) -> (String, String) {
        (e.account.id.clone(), "home".to_string())
    }

    fn home_feed() -> Feed {
        crate::social::demo::feeds().remove(0)
    }

    // spec: SOC-027
    #[test]
    fn new_posts_wait_without_doubles_and_go_above_the_list_newest_first() {
        let (mut st, e, fake, ctx) = ready();
        let key = home_key(&e);
        *fake.newer.lock().unwrap() = vec![Ok(fresh(&["n3", "n2"])), Ok(fresh(&["n5", "n4", "n3"])), Ok(fresh(&["home-k2", "n6"]))];
        for want in [2, 4, 5] {
            assert!(st.check(&ctx, &e, &home_feed()));
            wait(&mut st, |s| !s.timelines[&key].checking);
            assert_eq!(st.waiting_count(&e.account.id, "home"), want);
        }
        let t = &st.timelines[&key];
        assert_eq!(ids(&t.waiting), ["n6", "n5", "n4", "n3", "n2"], "newest first, n3 once, a post of the list is not waiting");
        assert_eq!(ids(&t.posts), ["home-k2", "home-k1"], "the list did not change");
        // the second check knew what waits
        let calls = fake.newer_calls.lock().unwrap();
        assert!(calls[1].1.known.iter().any(|k| k.starts_with("n3|")) && calls[1].1.known.iter().any(|k| k.starts_with("home-k1|")));
        drop(calls);
        // a waiting post follows the newest data of the server
        let mut edited = post("n4", "bob", 1, None);
        edited.text_markdown = "edited".into();
        *fake.newer.lock().unwrap() = vec![Ok(Fresh { posts: vec![edited], ..Default::default() })];
        st.check(&ctx, &e, &home_feed());
        wait(&mut st, |s| !s.timelines[&key].checking);
        let t = &st.timelines[&key];
        assert_eq!(t.waiting.len(), 5);
        assert_eq!(t.waiting.iter().find(|p| p.id == "n4").unwrap().text_markdown, "edited");
        // showing them: above the list, nothing twice, nothing waiting any more
        assert_eq!(st.show_waiting(&key), 5);
        let t = &st.timelines[&key];
        assert_eq!(ids(&t.posts), ["n6", "n5", "n4", "n3", "n2", "home-k2", "home-k1"]);
        assert!(t.waiting.is_empty());
        assert_eq!(st.show_waiting(&key), 0);
    }

    // spec: SOC-026
    #[test]
    fn a_check_that_hit_its_limit_leaves_a_gap_that_pages_fill_until_a_known_post() {
        let (mut st, e, fake, ctx) = ready();
        let key = home_key(&e);
        let mut f = fresh(&["n3", "n2"]);
        f.gap = Some("cur-a".into());
        *fake.newer.lock().unwrap() = vec![Ok(f)];
        st.check(&ctx, &e, &home_feed());
        wait(&mut st, |s| !s.timelines[&key].checking);
        st.show_waiting(&key);
        let gap = st.timelines[&key].gaps.clone();
        assert_eq!(gap.len(), 1, "{gap:?}");
        assert!(gap[0].above.starts_with("n2|") && gap[0].cursor == "cur-a");
        // the first page does not reach the list: the gap moves down
        fake.pages.lock().unwrap().insert("cur-a".into(), Page { posts: ["m3", "m2"].iter().map(|i| post(i, "bob", 1, None)).collect(), cursor: Some("cur-b".into()), top: None });
        fake.pages.lock().unwrap().insert("cur-b".into(), Page { posts: ["m1", "home-k2"].iter().map(|i| post(i, "bob", 1, None)).collect(), cursor: Some("cur-c".into()), top: None });
        st.fill_gap(&ctx, &e, &home_feed(), &gap[0].above);
        wait(&mut st, |s| !s.timelines[&key].filling);
        let t = &st.timelines[&key];
        assert_eq!(ids(&t.posts), ["n3", "n2", "m3", "m2", "home-k2", "home-k1"]);
        assert_eq!(t.gaps.len(), 1);
        assert!(t.gaps[0].above.starts_with("m2|") && t.gaps[0].cursor == "cur-b");
        // the second page meets a known post: the gap is closed
        let above = t.gaps[0].above.clone();
        st.fill_gap(&ctx, &e, &home_feed(), &above);
        wait(&mut st, |s| !s.timelines[&key].filling);
        let t = &st.timelines[&key];
        assert_eq!(ids(&t.posts), ["n3", "n2", "m3", "m2", "m1", "home-k2", "home-k1"]);
        assert!(t.gaps.is_empty());
        // a gap whose post is gone (the list was reloaded) is simply dropped
        let mut t2 = Timeline { gaps: vec![Gap { above: "zz|".into(), cursor: "c".into() }], ..Default::default() };
        t2.fill("zz|", Page::default());
        assert!(t2.gaps.is_empty());
    }

    // spec: SOC-025
    #[test]
    fn a_failed_check_keeps_the_list_waits_twice_as_long_and_a_good_one_clears_the_error() {
        let (mut st, e, fake, ctx) = ready();
        let key = home_key(&e);
        st.check_every = Duration::from_secs(60);
        assert_eq!([backoff(st.check_every, 0), backoff(st.check_every, 1), backoff(st.check_every, 2), backoff(st.check_every, 9)], [Duration::from_secs(60), Duration::from_secs(120), Duration::from_secs(240), Duration::from_secs(960)]);
        *fake.newer.lock().unwrap() = vec![Err("offline".into()), Err("offline".into()), Ok(fresh(&["n1"]))];
        for failures in [1, 2] {
            let before = Instant::now();
            st.check(&ctx, &e, &home_feed());
            wait(&mut st, |s| !s.timelines[&key].checking);
            let t = &st.timelines[&key];
            assert_eq!((t.failures, t.check_error.as_deref()), (failures, Some("offline")));
            assert_eq!(ids(&t.posts), ["home-k2", "home-k1"], "the old list stays");
            let wait_for = t.next_check.unwrap().duration_since(before);
            assert!(wait_for >= Duration::from_secs(60) * (1 << failures) && wait_for < Duration::from_secs(60) * (1 << failures) + Duration::from_secs(5), "{wait_for:?}");
        }
        st.check(&ctx, &e, &home_feed());
        wait(&mut st, |s| !s.timelines[&key].checking);
        let t = &st.timelines[&key];
        assert_eq!((t.failures, t.check_error.clone(), t.waiting.len()), (0, None, 1));
    }

    // spec: SOC-025
    #[test]
    fn only_feeds_open_in_a_tab_are_checked_once_each_when_their_time_has_come() {
        let (mut st, e, fake, ctx) = ready();
        let accounts = vec![e.clone()];
        st.check_every = Duration::from_secs(300);
        let at = |f: &str| FeedRef { account: e.account.id.clone(), feed: f.into() };
        let t0 = Instant::now();
        // the first look plans the first check
        st.tick(&ctx, &accounts, &[at("home")], t0);
        assert!(fake.newer_calls.lock().unwrap().is_empty());
        let planned = st.timelines[&home_key(&e)].next_check.unwrap();
        assert!(planned >= t0 + Duration::from_secs(300));
        assert!(st.timelines[&(e.account.id.clone(), "list:1".to_string())].next_check.is_none(), "a feed that is not open is not planned");
        // not yet
        st.tick(&ctx, &accounts, &[at("home")], planned - Duration::from_secs(1));
        assert!(fake.newer_calls.lock().unwrap().is_empty());
        // due: the feed open in two tabs is asked once, the feed that is not open never
        let due = planned + Duration::from_secs(1);
        st.tick(&ctx, &accounts, &[at("home"), at("home"), at(""), FeedRef { account: "gone".into(), feed: "home".into() }], due);
        wait(&mut st, |s| !s.timelines[&home_key(&e)].checking);
        let calls = fake.newer_calls.lock().unwrap();
        assert_eq!(calls.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), ["home"]);
        drop(calls);
        // the next check is planned again
        assert!(st.timelines[&home_key(&e)].next_check.unwrap() > Instant::now());
        // never when it is switched off
        st.check_every = Duration::ZERO;
        st.timelines.get_mut(&home_key(&e)).unwrap().next_check = Some(t0);
        st.tick(&ctx, &accounts, &[at("home")], due + Duration::from_secs(10_000));
        assert_eq!(fake.newer_calls.lock().unwrap().len(), 1);
    }

    // spec: SOC-025
    #[test]
    fn never_two_checks_of_one_feed_at_once_and_none_while_the_list_loads() {
        let (mut st, e, fake, ctx) = ready();
        let key = home_key(&e);
        let (release, gate) = std::sync::mpsc::channel();
        *fake.gate.lock().unwrap() = Some(gate);
        assert!(st.check(&ctx, &e, &home_feed()));
        assert!(!st.check(&ctx, &e, &home_feed()), "one check of this feed is running");
        st.check_every = Duration::from_secs(60);
        st.timelines.get_mut(&key).unwrap().next_check = Some(Instant::now());
        st.tick(&ctx, std::slice::from_ref(&e), &[FeedRef { account: e.account.id.clone(), feed: "home".into() }], Instant::now() + Duration::from_secs(1000));
        // the other feed is not held back by it
        assert!(st.check(&ctx, &e, &crate::social::demo::feeds().remove(1)));
        release.send(()).unwrap();
        release.send(()).unwrap();
        wait(&mut st, |s| s.timelines.values().all(|t| !t.checking));
        assert_eq!(fake.newer_calls.lock().unwrap().len(), 2, "one for each feed");
        // no check while the first page is on its way (or has not come)
        st.timelines.get_mut(&key).unwrap().loading = true;
        assert!(!st.check(&ctx, &e, &home_feed()));
        st.timelines.get_mut(&key).unwrap().loading = false;
        st.timelines.get_mut(&key).unwrap().loaded = false;
        assert!(!st.check(&ctx, &e, &home_feed()));
        // too many waiting posts pause the checks
        let t = st.timelines.get_mut(&key).unwrap();
        t.loaded = true;
        t.waiting = (0..MAX_WAITING).map(|i| post(&format!("w{i}"), "bob", 1, None)).collect();
        assert!(!st.check(&ctx, &e, &home_feed()));
    }

    // spec: SOC-029
    #[test]
    fn a_first_page_replaces_the_list_and_drops_what_waited() {
        let (mut st, e, _fake, _ctx) = ready();
        let key = home_key(&e);
        {
            let t = st.timelines.get_mut(&key).unwrap();
            t.waiting = vec![post("w", "bob", 1, None)];
            t.gaps = vec![Gap { above: "x".into(), cursor: "c".into() }];
            t.top = Some("5".into());
        }
        st.tx.send(Msg::Page { key: key.clone(), client: None, replace: true, result: Ok(Page { posts: vec![post("fresh", "bob", 1, None)], cursor: None, top: Some("9".into()) }) }).unwrap();
        st.poll();
        let t = &st.timelines[&key];
        assert_eq!((ids(&t.posts), t.waiting.len(), t.gaps.len(), t.top.as_deref(), t.reloads), (vec!["fresh"], 0, 0, Some("9"), 1));
        // an older page does not touch any of it
        st.timelines.get_mut(&key).unwrap().waiting = vec![post("w", "bob", 1, None)];
        st.tx.send(Msg::Page { key: key.clone(), client: None, replace: false, result: Ok(Page { posts: vec![post("old", "bob", 1, None)], cursor: Some("c".into()), top: Some("1".into()) }) }).unwrap();
        st.poll();
        let t = &st.timelines[&key];
        assert_eq!((ids(&t.posts), t.waiting.len(), t.top.as_deref()), (vec!["fresh", "old"], 1, Some("9")));
    }
}
