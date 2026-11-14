//! What the feed view remembers: the chosen account and feed, the loaded posts of every feed,
//! and the background jobs (feeds, pages, saving a post). Jobs send their result over a channel
//! that `poll` reads each frame; a job that panics sends an error instead, so no spinner runs
//! forever (ADR-0006).

use super::images::ImageCache;
use super::note::SavedPost;
use super::{Client, Feed, Page, Post, SocialAccountEntry};
use eframe::egui::{Context, Rect};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

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
}

enum Msg {
    Feeds { account: String, client: Option<Client>, result: Result<Vec<Feed>, String> },
    Page { key: (String, String), client: Option<Client>, replace: bool, result: Result<Page, String> },
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
        let (client, feed, key2) = (self.client_source(e), feed.clone(), key.clone());
        run(
            ctx,
            &self.tx,
            move || {
                let mut c = client();
                let result = c.timeline(&feed, cursor.as_deref());
                Msg::Page { key, client: Some(c), replace, result }
            },
            move |m| Msg::Page { key: key2, client: None, replace, result: Err(m) },
        );
    }

    /// Start the first page of a feed that was never loaded.
    pub fn ensure_timeline(&mut self, ctx: &Context, e: &SocialAccountEntry, feed: &Feed) {
        if self.demo {
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
        if let Some(c) = cursor.filter(|_| !self.demo) {
            self.load(ctx, e, feed, Some(c), false);
        }
    }

    /// Start again from the newest post (the old list stays until the new one arrives).
    pub fn refresh(&mut self, ctx: &Context, e: &SocialAccountEntry, feed: &Feed) {
        if self.demo {
            return;
        }
        if let Some(t) = self.timelines.get_mut(&(e.account.id.clone(), feed.id.clone())) {
            t.error = None;
        }
        self.load(ctx, e, feed, None, true);
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
        self.timelines.insert((id, "home".into()), Timeline { posts: super::demo::posts(), cursor: None, loading: false, loaded: true, error: None });
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
        let page = |ids: &[&str], cursor: Option<&str>| Page { posts: ids.iter().map(|i| post(i, "bob", 1, None)).collect(), cursor: cursor.map(String::from) };
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
}
