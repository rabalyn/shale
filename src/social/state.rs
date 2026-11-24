//! What the feed view remembers: the chosen account and feed, the loaded posts of every feed,
//! and the background jobs (feeds, pages, saving a post). Jobs send their result over a channel
//! that `poll` reads each frame; a job that panics sends an error instead, so no spinner runs
//! forever (ADR-0006).

use super::actions::{ActionError, Draft, InstanceInfo, Reacted, Reaction, Sent, Writes};
use super::compose::Compose;
use super::images::ImageCache;
use super::note::SavedPost;
use super::poll::Poll;
use super::post_index::PostIndex;
use super::schedule::{Scheduled, ScheduledState};
use super::thread::{ThreadStatus, Threads};
use super::{Client, Feed, FeedRef, Fresh, Network, Page, Post, Since, SocialAccountEntry, ThreadView};
use crate::busy::{Busy, Cancel, Key, Task, LISTED_AFTER};
use eframe::egui::{Context, Rect};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The tasks of the feeds (JOB-035, JOB-036): the first page or the next page of a feed, a check
/// for new posts, filling a gap, and fetching a post for saving. One of each per feed (per post).
pub const FEED_LOAD: Key = Key::new("social-feed-load");
pub const FEED_CHECK: Key = Key::new("social-feed-check");
pub const FEED_GAP: Key = Key::new("social-feed-gap");
pub const POST_SAVE: Key = Key::new("social-post-save");

/// What a request that Cancel stopped reports instead of a result.
pub const STOPPED: &str = "stopped";

/// The subject of the task of a feed of an account.
pub fn feed_subject(account: &str, feed: &str) -> String {
    format!("{account}\u{1e}{feed}")
}

/// The request `f`, unless Cancel was asked for before it starts; and its result is dropped when
/// Cancel came while it ran: the only safe points of one request are before and after it.
pub(super) fn unless_stopped<T>(cancel: &Cancel, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    if cancel.is_cancelled() {
        return Err(STOPPED.into());
    }
    let r = f();
    if cancel.is_cancelled() {
        Err(STOPPED.into())
    } else {
        r
    }
}

/// How many posts may wait to be shown; then the checks pause (SOC-026).
pub const MAX_WAITING: usize = 1000;

/// Where the posts come from when it is not a real account: screenshots (`demo`) and tests plug
/// in a pretend one. The jobs ask it instead of a `Client`.
pub trait Source: Send + Sync {
    /// The thread and the files of a post for saving; `None` when `stop` says so at a safe point (JOB-036).
    fn save(&self, post: &Post, _stop: &dyn Fn() -> bool) -> Option<SavedPost> {
        Some(super::demo::saved(post))
    }
    fn page(&self, feed: &Feed, cursor: Option<&str>) -> Result<Page, String>;
    fn newer(&self, feed: &Feed, since: &Since) -> Result<Fresh, String>;
    /// One post by its id (SOC-039).
    fn post(&self, id: &str) -> Result<Post, String>;
    /// The conversation around a post (SOC-045).
    fn thread(&self, post: &Post) -> Result<ThreadView, String>;
    /// Like / boost a post or undo it (SOC-050); a pretend server that does not say fails.
    fn react(&self, _post: &Post, _kind: Reaction, _on: bool) -> Result<Reacted, ActionError> {
        Err("this pretend server does not take writes".into())
    }
    /// Send a post (SOC-051).
    fn send(&self, _draft: &Draft) -> Result<Sent, ActionError> {
        Err("this pretend server does not take writes".into())
    }
    /// Vote in a poll (SOC-062).
    fn vote(&self, _poll: &Poll, _choices: &[usize]) -> Result<Poll, ActionError> {
        Err("this pretend server does not take writes".into())
    }
    /// The scheduled posts (SOC-064).
    fn scheduled(&self) -> Result<Vec<Scheduled>, String> {
        Ok(vec![])
    }
    /// Delete a scheduled post (SOC-064).
    fn cancel_scheduled(&self, _id: &str) -> Result<(), ActionError> {
        Err("this pretend server does not take writes".into())
    }
    /// Tell the server that the notifications up to `mark` were read (SOC-079).
    fn mark_read(&self, _mark: &str) -> Result<(), String> {
        Ok(())
    }
    /// What the server allows in a post (SOC-052).
    fn instance(&self) -> Result<InstanceInfo, String> {
        Ok(InstanceInfo::defaults(Network::Mastodon))
    }
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
    /// when a check (or the first page) last went well
    pub last_checked: Option<Instant>,
    /// checks that failed in a row
    pub failures: u32,
    /// when the next check is due; None: not planned yet
    pub next_check: Option<Instant>,
    /// how often the list was replaced by a first page (a view that sees a new number starts at the top)
    pub reloads: u32,
    /// Notifications: the `rank` of the newest notification the server was told is read (SOC-079)
    pub marked: i64,
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
        // a group that this check cut from the one that waits is one entry again (SOC-078)
        if let (Some(last), Some(first)) = (fresh_new.last(), self.waiting.first()) {
            if super::notifications::same_group(last, first) && !self.gaps.iter().any(|g| g.above == post_key(last)) {
                let last = fresh_new.pop().expect("not empty");
                super::notifications::merge_into(&mut self.waiting[0], last);
            }
        }
        fresh_new.append(&mut self.waiting);
        self.waiting = fresh_new;
        if fresh.top.is_some() {
            self.top = fresh.top;
        }
    }

    /// Put a post that the feed does not hold (any more) into the list where its date puts it,
    /// newest first. Below the last post loaded it goes at the end, and the pages between are
    /// left as a gap that "Load more in between" fills. A post the list holds stays where it is.
    fn insert_found(&mut self, post: Post) {
        let k = post_key(&post);
        if self.posts.iter().any(|p| post_key(p) == k) {
            return;
        }
        let at = self.posts.iter().position(|p| p.created_at < post.created_at).unwrap_or(self.posts.len());
        if at == self.posts.len() && !self.posts.is_empty() {
            if let Some(cursor) = self.cursor.take() {
                self.gaps.push(Gap { above: post_key(self.posts.last().expect("not empty")), cursor });
            }
        }
        self.posts.insert(at, post);
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
        // a group cut by the gap is one entry again (SOC-078)
        if new.first().is_some_and(|f| super::notifications::same_group(&self.posts[at], f)) {
            let first = new.remove(0);
            super::notifications::merge_into(&mut self.posts[at], first);
        }
        if let (Some(last), Some(below)) = (new.last(), self.posts.get(at + 1)) {
            if super::notifications::same_group(below, last) && !self.gaps.iter().any(|g| g.above == post_key(last)) {
                let last = new.pop().expect("not empty");
                super::notifications::merge_into(&mut self.posts[at + 1], last);
            }
        }
        self.posts.splice(at + 1..at + 1, new);
    }
}

/// A search hit waiting for the feed of its tab to show.
pub struct Focus {
    pub tab: u64,
    pub account: String,
    pub post: Post,
}

pub(super) enum Msg {
    Feeds { account: String, client: Option<Client>, result: Result<Vec<Feed>, String> },
    Page { key: (String, String), client: Option<Client>, replace: bool, result: Result<Page, String> },
    Check { key: (String, String), client: Option<Client>, result: Result<Fresh, String> },
    Gap { key: (String, String), above: String, client: Option<Client>, result: Result<Page, String> },
    Saved { post: String, account: String, feed: String, label: &'static str, result: Result<SavedPost, String> },
    Thread { account: String, feed: String, id: String, client: Option<Client>, result: Result<Box<ThreadView>, String> },
    Fetched { tab: u64, account: String, feed: String, client: Option<Client>, result: Result<Box<Post>, String> },
    Reacted { account: String, post: String, kind: Reaction, on: bool, client: Option<Client>, result: Result<Reacted, ActionError> },
    Sent { account: String, feed: String, client: Option<Client>, result: Result<Sent, ActionError> },
    Voted { account: String, post: String, client: Option<Client>, result: Result<Poll, ActionError> },
    ScheduledList { account: String, client: Option<Client>, result: Result<Vec<Scheduled>, String> },
    ScheduledCancelled { account: String, id: String, client: Option<Client>, result: Result<(), ActionError> },
    Instance { account: String, client: Option<Client>, result: Result<InstanceInfo, String> },
    Marked { account: String, client: Option<Client>, result: Result<(), String> },
    Picked { result: Result<Vec<std::path::PathBuf>, String> },
}

pub struct SocialState {
    /// the feed shown last for each account (feed ids): what a tab shows that names no feed
    pub feed_of: HashMap<String, String>,
    pub feeds: HashMap<String, FeedsState>,
    pub timelines: HashMap<(String, String), Timeline>,
    /// what runs in the background: the window's registry (JOB-035)
    pub busy: Busy,
    /// posts being fetched for saving (see `post_key`)
    pub saving: HashSet<String>,
    /// posts whose content warning was opened
    pub revealed: HashSet<String>,
    /// the posts are made-up ones (screenshots, tests): nothing goes to the network
    pub demo: bool,
    pub images: Option<ImageCache>,
    /// the videos and GIFs of posts, in the same cache folder (SOC-076)
    pub clips: Option<super::clips::Clips>,
    pub cache_dir: PathBuf,
    /// finished saves waiting for the Import dialog: the post, thread and media, and the network
    pub saved: Vec<(SavedPost, &'static str)>,
    /// messages for the user (failed saves)
    pub messages: Vec<String>,
    /// where the controls are (for tests)
    pub rects: Vec<(String, Rect)>,
    /// where the posts of the list shown are, by `post_key` (for tests)
    pub post_rects: Vec<(String, Rect)>,
    /// how high each post was drawn at a width, and whether it came out the same twice (PERF-006): a post far from the view is not laid out again
    pub card_heights: std::collections::HashMap<(String, u32), (f32, bool)>,
    /// where the post rows of the sidebar search are (for tests)
    pub hit_rects: Vec<(usize, Rect)>,
    /// the screen rectangle of the list's viewport (for tests)
    pub list_rect: Rect,
    /// how often the feeds open in tabs are checked for new posts (zero: never)
    pub check_every: Duration,
    /// the posts of a pretend server (screenshots, tests); None: the real account
    pub source: Option<Arc<dyn Source>>,
    /// where each tab's view is held still while posts come in above it (see `feed_ui`)
    pub anchors: HashMap<u64, super::feed_ui::Anchor>,
    /// every post loaded is searchable (SOC-030)
    pub index: PostIndex,
    /// a search hit that a tab is to show (SOC-035)
    pub focus: Option<Focus>,
    /// hits whose post has to be fetched first (tab, account, feed, post id), started by `start_fetches` (SOC-039)
    pub fetch_wanted: Vec<(u64, String, String, String)>,
    /// the Settings dialog asked "Clear the post search index?" and waits for the answer (SOC-040)
    pub confirm_clear: bool,
    /// the picture viewer open over a tab (SOC-044)
    pub viewer: Option<super::viewer::Viewer>,
    /// the conversations opened from posts (SOC-045)
    pub threads: Threads,
    /// reactions on their way, accounts that cannot write, what the servers allow (SOC-050, SOC-056)
    pub writes: Writes,
    /// the compose box (SOC-051)
    pub compose: Option<Compose>,
    /// pictures coming into the compose box: the file dialog, drops (SOC-058)
    pub intake: super::compose_pictures::Intake,
    /// the options ticked in the polls on screen, before the vote is sent (SOC-062)
    pub polls: super::poll::Selected,
    /// the window of the scheduled posts (SOC-064)
    pub scheduled: ScheduledState,
    /// the language a new post starts with (the setting "Post language", SOC-065)
    pub post_language: String,
    /// the calendar week starts on Monday (the date picker of the schedule uses the calendar's, SOC-063)
    pub week_monday: bool,
    pub(super) clients: HashMap<String, Client>,
    pub(super) tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

/// Identifies a post in the feed: the same post boosted by two people shows twice.
pub fn post_key(p: &Post) -> String {
    let boost = p.reposted_by.as_ref().map(|b| b.id.as_str()).unwrap_or("");
    // a notification entry is told from the same post in another entry (SOC-078)
    match &p.notice {
        Some(n) => format!("{}|{boost}|{}", p.id, n.id),
        None => format!("{}|{boost}", p.id),
    }
}

pub(super) fn run(ctx: &Context, tx: &Sender<Msg>, job: impl FnOnce() -> Msg + Send + 'static, fail: impl FnOnce(String) -> Msg + Send + 'static) {
    let (ctx, tx) = (ctx.clone(), tx.clone());
    crate::jobs::run(job, move |r| {
        let msg = r.unwrap_or_else(fail);
        let _ = tx.send(msg);
        ctx.request_repaint();
    });
}

/// [`run`] under a task of the busy registry that Cancel can stop (JOB-035): the task ends before
/// the message is sent, so a result that has arrived always finds the action idle.
pub(super) fn run_task(ctx: &Context, tx: &Sender<Msg>, mut task: Task, job: impl FnOnce(&Cancel) -> Msg + Send + 'static, fail: impl FnOnce(String) -> Msg + Send + 'static) {
    let (ctx, tx, cancel) = (ctx.clone(), tx.clone(), task.cancellable());
    crate::jobs::run(
        move || job(&cancel),
        move |r| {
            drop(task);
            let msg = r.unwrap_or_else(fail);
            let _ = tx.send(msg);
            ctx.request_repaint();
        },
    );
}

impl SocialState {
    pub fn new(cache_dir: PathBuf) -> SocialState {
        let (tx, rx) = channel();
        SocialState {
            feed_of: HashMap::new(),
            feeds: HashMap::new(),
            timelines: HashMap::new(),
            busy: Busy::default(),
            saving: HashSet::new(),
            revealed: HashSet::new(),
            demo: false,
            images: None,
            clips: None,
            cache_dir,
            saved: vec![],
            messages: vec![],
            rects: vec![],
            post_rects: vec![],
            card_heights: Default::default(),
            hit_rects: vec![],
            list_rect: Rect::NOTHING,
            check_every: Duration::from_secs(300),
            source: None,
            anchors: HashMap::new(),
            index: PostIndex::in_memory(),
            focus: None,
            fetch_wanted: vec![],
            confirm_clear: false,
            viewer: None,
            threads: Threads::default(),
            writes: Writes::default(),
            compose: None,
            intake: Default::default(),
            polls: Default::default(),
            scheduled: Default::default(),
            post_language: super::language::system(),
            week_monday: true,
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

    /// Keep the posts in a file (the real app) instead of in memory; without one the feeds work
    /// as before and the search finds only what this run loaded.
    pub fn open_index(&mut self, path: &std::path::Path) {
        if let Some(i) = PostIndex::open(path) {
            self.index = i;
        }
    }

    /// Show a search hit: the next time the tab shows the feed it is scrolled to the post, which
    /// is put into the list from the index when the feed no longer has it (SOC-035).
    pub fn show_hit(&mut self, tab: u64, hit: &super::post_index::PostHit) {
        // the row of the search may be older than the index: take the post as the index has it now (SOC-059)
        let post = self.index.get(&hit.account, &hit.key).unwrap_or_else(|| hit.post.clone());
        self.focus = Some(Focus { tab, account: hit.account.clone(), post });
    }

    /// The post a tab is to be scrolled to, once its feed has loaded: it is in the list by now.
    /// None while the list loads (or when there is nothing to show).
    pub fn take_focus(&mut self, tab: u64, key: &(String, String)) -> Option<String> {
        let k = self.focus.as_ref().filter(|f| f.tab == tab && f.account == key.0).map(|f| post_key(&f.post))?;
        let t = self.timelines.get_mut(key)?;
        if !t.loaded {
            if t.error.is_some() {
                self.focus = None;
            }
            return None;
        }
        if t.waiting.iter().any(|w| post_key(w) == k) {
            self.show_waiting(key);
        }
        let f = self.focus.take()?;
        // the copy in memory is the newer one: it has the reactions made since the post was indexed
        let post = self.live_copy(&f.account, &f.post).unwrap_or(f.post);
        self.timelines.get_mut(key)?.insert_found(post);
        Some(k)
    }

    /// Every post the feeds and conversations of the account hold in memory (not the quoted ones).
    fn held<'a>(&'a self, account: &'a str) -> impl Iterator<Item = &'a Post> + Clone {
        let lists = self.timelines.iter().filter(move |((a, _), _)| a == account).flat_map(|(_, t)| t.posts.iter().chain(t.waiting.iter()));
        let threads = self.threads.map.iter().filter(move |((a, _), _)| a == account).flat_map(|(_, e)| {
            let ready = match &e.status {
                super::thread::ThreadStatus::Ready(v) => Some(v),
                _ => None,
            };
            std::iter::once(&e.post).chain(ready.into_iter().flat_map(|v| std::iter::once(&v.post).chain(v.ancestors.iter()).chain(v.replies.iter().map(|r| &r.post))))
        });
        lists.chain(threads)
    }

    /// The post as a feed or conversation of the account holds it now (SOC-059): the same post
    /// (and boost) in any list, else the same post in a conversation or quoted by another one.
    fn live_copy(&self, account: &str, like: &Post) -> Option<Post> {
        let k = post_key(like);
        let mut all = self.held(account);
        let found = all.clone().find(|p| post_key(p) == k).or_else(|| all.find_map(|p| p.quote.as_deref().filter(|q| q.id == like.id)));
        found.map(|p| Post { reposted_by: like.reposted_by.clone(), ..p.clone() })
    }

    /// Posts that came in with quotes whose state is not known (SOC-060) take the state from a
    /// copy of the quoted post that a feed or conversation holds and knows.
    pub(super) fn adopt_known<'a>(&self, account: &str, posts: impl Iterator<Item = &'a mut Post>) {
        let mut want: Vec<&mut Post> = posts.filter(|p| p.quote.as_deref().is_some_and(|q| q.mine.unknown)).collect();
        if want.is_empty() {
            return;
        }
        let known: HashMap<&str, &Post> = self.held(account).flat_map(|p| std::iter::once(p).chain(p.quote.as_deref())).filter(|p| !p.mine.unknown).map(|p| (p.id.as_str(), p)).collect();
        for q in want.iter_mut().filter_map(|p| p.quote.as_deref_mut()) {
            if let Some(k) = known.get(q.id.as_str()) {
                (q.mine, q.reply_count, q.boost_count, q.like_count) = (k.mine.clone(), k.reply_count, k.boost_count, k.like_count);
            }
        }
    }

    /// Start the fetches that `open_post_hit` asked for: each post by its id, in a background job.
    pub fn start_fetches(&mut self, ctx: &Context, accounts: &[SocialAccountEntry]) {
        for (tab, account, feed, id) in std::mem::take(&mut self.fetch_wanted) {
            let Some(e) = accounts.iter().find(|e| e.account.id == account) else { continue };
            let (client, src) = (self.client_source(e), self.source.clone());
            let (account2, feed2) = (account.clone(), feed.clone());
            run(
                ctx,
                &self.tx,
                move || {
                    let (client, result) = match src {
                        Some(s) => (None, s.post(&id).map(Box::new)),
                        None => {
                            let mut c = client();
                            let r = c.fetch_post(&id).map(Box::new);
                            (Some(c), r)
                        }
                    };
                    Msg::Fetched { tab, account, feed, client, result }
                },
                move |m| Msg::Fetched { tab, account: account2, feed: feed2, client: None, result: Err(m) },
            );
        }
    }

    /// Forget everything about an account (removed).
    pub fn forget(&mut self, account: &str) {
        self.clients.remove(account);
        self.feeds.remove(account);
        self.feed_of.remove(account);
        self.timelines.retain(|k, _| k.0 != account);
        self.threads.forget(account);
    }

    /// The signed-in client of an account is dropped: the next request signs in with the secret
    /// the account has now (it was replaced, SOC-043).
    pub fn forget_client(&mut self, account: &str) {
        self.clients.remove(account);
    }

    pub(super) fn client_source(&self, e: &SocialAccountEntry) -> impl FnOnce() -> Client + Send + 'static {
        let (cached, entry) = (self.clients.get(&e.account.id).cloned(), e.clone());
        move || cached.unwrap_or_else(|| Client::new(entry.account.clone(), entry.secret().expose()))
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
    /// The first page is a task listed at once, the next one (it follows the scrolling) after 2 s (JOB-035).
    fn load(&mut self, ctx: &Context, e: &SocialAccountEntry, feed: &Feed, cursor: Option<String>, replace: bool) {
        let key = (e.account.id.clone(), feed.id.clone());
        if self.timelines.get(&key).is_some_and(|t| t.loading) {
            return;
        }
        let (label, wait) = match cursor {
            Some(_) => (format!("Loading more posts of “{}”…", feed.title), LISTED_AFTER),
            None => (format!("Loading “{}” of @{}…", feed.title, e.account.handle), Duration::ZERO),
        };
        let Some(task) = self.busy.begin_listed_after(FEED_LOAD.with(&feed_subject(&key.0, &key.1)), label, wait) else { return };
        let t = self.timelines.entry(key.clone()).or_default();
        t.loading = true;
        t.error = None;
        let (client, src, feed, key2) = (self.client_source(e), self.source.clone(), feed.clone(), key.clone());
        run_task(
            ctx,
            &self.tx,
            task,
            move |cancel| {
                let (client, result) = match src {
                    Some(s) => (None, unless_stopped(cancel, || s.page(&feed, cursor.as_deref()))),
                    None => {
                        let mut c = client();
                        let r = unless_stopped(cancel, || c.timeline(&feed, cursor.as_deref()));
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
        self.check_as(ctx, e, feed, false)
    }

    /// [`SocialState::check`]; `quiet`: the timer asked, so the indicator lists the task only
    /// when it runs longer than 2 s (JOB-035).
    fn check_as(&mut self, ctx: &Context, e: &SocialAccountEntry, feed: &Feed, quiet: bool) -> bool {
        if self.demo && self.source.is_none() {
            return false;
        }
        let key = (e.account.id.clone(), feed.id.clone());
        let Some(t) = self.timelines.get_mut(&key) else { return false };
        if !t.loaded || t.loading || t.checking || t.waiting.len() >= MAX_WAITING {
            return false;
        }
        let wait = if quiet { LISTED_AFTER } else { Duration::ZERO };
        let Some(task) = self.busy.begin_listed_after(FEED_CHECK.with(&feed_subject(&key.0, &key.1)), format!("Checking the feed of @{}…", e.account.handle), wait) else { return false };
        t.checking = true;
        // the keys of the entries and, for notifications, the ids of each notification they hold
        let since = Since { top: t.top.clone(), known: t.posts.iter().chain(&t.waiting).flat_map(|p| std::iter::once(post_key(p)).chain(p.notice.iter().flat_map(|n| n.ids.clone()))).collect() };
        let (client, src, feed, key2) = (self.client_source(e), self.source.clone(), feed.clone(), key.clone());
        run_task(
            ctx,
            &self.tx,
            task,
            move |cancel| {
                let (client, result) = match src {
                    Some(s) => (None, unless_stopped(cancel, || s.newer(&feed, &since))),
                    None => {
                        let mut c = client();
                        let r = unless_stopped(cancel, || c.newer(&feed, &since));
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
                    self.check_as(ctx, e, &feed, true);
                }
                _ => {}
            }
        }
    }

    /// Put the waiting posts of a feed above its list. The number of posts put in.
    pub fn show_waiting(&mut self, key: &(String, String)) -> usize {
        let Some(t) = self.timelines.get_mut(key) else { return 0 };
        let have: HashSet<String> = t.posts.iter().map(post_key).collect();
        let mut fresh: Vec<Post> = std::mem::take(&mut t.waiting).into_iter().filter(|p| !have.contains(&post_key(p))).collect();
        // a group that a check cut from the one in the list is one entry again (SOC-078)
        if let (Some(last), Some(first)) = (fresh.last(), t.posts.first()) {
            if super::notifications::same_group(last, first) && !t.gaps.iter().any(|g| g.above == post_key(last)) {
                let last = fresh.pop().expect("not empty");
                super::notifications::merge_into(&mut t.posts[0], last);
            }
        }
        let n = fresh.len();
        t.posts.splice(0..0, fresh);
        n
    }

    /// How many new posts wait for a feed; of notifications the unread ones (SOC-079).
    pub fn waiting_count(&self, account: &str, feed: &str) -> usize {
        let unread = |p: &&Post| p.notice.as_ref().is_none_or(|n| n.unread);
        self.timelines.get(&(account.to_string(), feed.to_string())).map(|t| t.waiting.iter().filter(unread).count()).unwrap_or(0)
    }

    /// The Notifications feed is on screen: tell the server that what the list shows is read
    /// (SOC-079). Only when the newest entry shown was unread and was not sent before; entries
    /// that wait behind the "new" button are not read yet. A failure is said once.
    pub fn read_notifications(&mut self, ctx: &Context, e: &SocialAccountEntry, feed: &Feed) {
        let key = (e.account.id.clone(), feed.id.clone());
        let Some(t) = self.timelines.get_mut(&key).filter(|t| t.loaded) else { return };
        let Some(n) = super::notifications::newest(&t.posts).filter(|n| n.unread && n.rank > t.marked) else { return };
        let (mark, rank) = (n.mark.clone(), n.rank);
        t.marked = rank;
        let (client, src, account) = (self.client_source(e), self.source.clone(), e.account.id.clone());
        let account2 = account.clone();
        run(
            ctx,
            &self.tx,
            move || {
                let (client, result) = match src {
                    Some(s) => (None, s.mark_read(&mark)),
                    None => {
                        let mut c = client();
                        let r = c.mark_notifications(&mark);
                        (Some(c), r)
                    }
                };
                Msg::Marked { account, client, result }
            },
            move |m| Msg::Marked { account: account2, client: None, result: Err(m) },
        );
    }

    /// Fetch the page that fills the gap below the post `above`.
    pub fn fill_gap(&mut self, ctx: &Context, e: &SocialAccountEntry, feed: &Feed, above: &str) {
        let key = (e.account.id.clone(), feed.id.clone());
        let Some(t) = self.timelines.get_mut(&key) else { return };
        let Some(gap) = t.gaps.iter().find(|g| g.above == above).cloned() else { return };
        if t.filling {
            return;
        }
        let Some(task) = self.busy.begin(FEED_GAP.with(&feed_subject(&key.0, &key.1)), format!("Loading the posts missing in “{}”…", feed.title)) else { return };
        t.filling = true;
        let (client, src, feed, key2, above2) = (self.client_source(e), self.source.clone(), feed.clone(), key.clone(), above.to_string());
        run_task(
            ctx,
            &self.tx,
            task,
            move |cancel| {
                let (client, result) = match src {
                    Some(s) => (None, unless_stopped(cancel, || s.page(&feed, Some(&gap.cursor)))),
                    None => {
                        let mut c = client();
                        let r = unless_stopped(cancel, || c.timeline(&feed, Some(&gap.cursor)));
                        (Some(c), r)
                    }
                };
                Msg::Gap { key, above: gap.above, client, result }
            },
            move |m| Msg::Gap { key: key2, above: above2, client: None, result: Err(m) },
        );
    }

    /// Start the job for a conversation that was opened and not fetched yet (SOC-045). `feed` is
    /// the feed of the tab: the posts that come are indexed with it.
    pub fn fetch_thread(&mut self, ctx: &Context, e: &SocialAccountEntry, feed: &str, id: &str) {
        let key = (e.account.id.clone(), id.to_string());
        if !self.threads.map.get(&key).is_some_and(|t| matches!(t.status, ThreadStatus::Wanted)) {
            return;
        }
        let (client, src) = (self.client_source(e), self.source.clone());
        let Some(t) = self.threads.map.get_mut(&key) else { return };
        t.status = ThreadStatus::Loading;
        let post = t.post.clone();
        let (account, feed, id) = (e.account.id.clone(), feed.to_string(), id.to_string());
        let (account2, feed2, id2) = (account.clone(), feed.clone(), id.clone());
        run(
            ctx,
            &self.tx,
            move || {
                let (client, result) = match src {
                    Some(s) => (None, s.thread(&post)),
                    None => {
                        let mut c = client();
                        let r = c.thread_view(&post);
                        (Some(c), r)
                    }
                };
                Msg::Thread { account, feed, id, client, result: result.map(Box::new) }
            },
            move |m| Msg::Thread { account: account2, feed: feed2, id: id2, client: None, result: Err(m) },
        );
    }

    /// Fetch the thread and the media of a post for the Import dialog.
    pub fn save(&mut self, ctx: &Context, e: &SocialAccountEntry, feed: &str, post: &Post) {
        let key = post_key(post);
        if self.saving.contains(&key) {
            return;
        }
        let Some(task) = self.busy.begin(POST_SAVE.with(&key), format!("Saving the post of @{}…", post.author.handle)) else { return };
        self.saving.insert(key.clone());
        let (client, src, post, label, demo, key2) = (self.client_source(e), self.source.clone(), post.clone(), e.account.network.label(), self.demo, key.clone());
        let (account, feed) = (e.account.id.clone(), feed.to_string());
        let (account2, feed2) = (account.clone(), feed.clone());
        run_task(
            ctx,
            &self.tx,
            task,
            move |cancel| {
                // Cancel is looked at before the thread and before each file; nothing is kept when it stops (JOB-036)
                let stop = || cancel.is_cancelled();
                let result = match src {
                    Some(s) => s.save(&post, &stop),
                    None if demo => Some(super::demo::saved(&post)),
                    None => client().save(&post, &stop),
                };
                let result = result.filter(|_| !stop()).ok_or_else(|| STOPPED.to_string());
                Msg::Saved { post: key, account, feed, label, result }
            },
            move |m| Msg::Saved { post: key2, account: account2, feed: feed2, label, result: Err(m) },
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
                Msg::Page { key, client, replace, mut result } => {
                    if let Some(c) = client {
                        self.clients.insert(key.0.clone(), c);
                    }
                    if let Ok(page) = &mut result {
                        self.adopt_known(&key.0, page.posts.iter_mut());
                    }
                    if let Ok(page) = &result {
                        self.index.add(&key.0, &key.1, &page.posts);
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
                                t.marked = 0;
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
                                let mut fresh = fresh;
                                // a group that the page boundary cut is one entry again (SOC-078)
                                if let (Some(last), Some(first)) = (t.posts.last(), fresh.first()) {
                                    if super::notifications::same_group(last, first) && !t.gaps.iter().any(|g| g.above == post_key(last)) {
                                        let first = fresh.remove(0);
                                        let at = t.posts.len() - 1;
                                        super::notifications::merge_into(&mut t.posts[at], first);
                                    }
                                }
                                t.posts.extend(fresh);
                            }
                            t.loaded = true;
                            t.last_checked.get_or_insert_with(Instant::now);
                        }
                        Err(e) if e == STOPPED => t.error = Some("loading was stopped".into()),
                        Err(e) => t.error = Some(e),
                    }
                }
                Msg::Check { key, client, mut result } => {
                    if let Some(c) = client {
                        self.clients.insert(key.0.clone(), c);
                    }
                    if let Ok(fresh) = &mut result {
                        self.adopt_known(&key.0, fresh.posts.iter_mut());
                    }
                    if let Ok(fresh) = &result {
                        self.index.add(&key.0, &key.1, &fresh.posts);
                    }
                    let every = self.check_every;
                    let t = self.timelines.entry(key).or_default();
                    t.checking = false;
                    match result {
                        Ok(fresh) => {
                            t.failures = 0;
                            t.check_error = None;
                            t.last_checked = Some(Instant::now());
                            t.next_check = Some(Instant::now() + every);
                            t.absorb(fresh);
                        }
                        // a check that was stopped is not a failure: the next one comes on time
                        Err(e) if e == STOPPED => t.next_check = Some(Instant::now() + every),
                        Err(e) => {
                            t.failures += 1;
                            t.check_error = Some(e);
                            t.next_check = Some(Instant::now() + backoff(every, t.failures));
                        }
                    }
                }
                Msg::Gap { key, above, client, mut result } => {
                    if let Some(c) = client {
                        self.clients.insert(key.0.clone(), c);
                    }
                    if let Ok(page) = &mut result {
                        self.adopt_known(&key.0, page.posts.iter_mut());
                    }
                    if let Ok(page) = &result {
                        self.index.add(&key.0, &key.1, &page.posts);
                    }
                    let t = self.timelines.entry(key).or_default();
                    t.filling = false;
                    match result {
                        Ok(page) => t.fill(&above, page),
                        Err(e) if e == STOPPED => {}
                        Err(e) => t.check_error = Some(e),
                    }
                }
                Msg::Saved { post, account, feed, label, result } => {
                    self.saving.remove(&post);
                    match result {
                        Ok(s) => {
                            // the thread and the replies of other people are posts of the account too (SOC-037)
                            let all: Vec<Post> = s.thread.iter().rev().cloned().chain(s.replies.iter().map(|r| r.post.clone())).collect();
                            self.index.add(&account, &feed, &all);
                            self.saved.push((s, label));
                        }
                        Err(e) if e == STOPPED => self.messages.push("Saving the post was stopped.".into()),
                        Err(e) => self.messages.push(format!("Could not save the post: {e}")),
                    }
                }
                Msg::Thread { account, feed, id, client, mut result } => {
                    if let Some(c) = client {
                        self.clients.insert(account.clone(), c);
                    }
                    if let Ok(v) = &mut result {
                        self.adopt_known(&account, v.posts_mut());
                    }
                    // the conversation's posts are posts of the account too (SOC-045, SOC-037)
                    let posts = self.threads.arrived(&account, &id, result);
                    self.index.add(&account, &feed, &posts);
                }
                Msg::Reacted { account, post, kind, on, client, result } => {
                    if let Some(c) = client {
                        self.clients.insert(account.clone(), c);
                    }
                    self.reacted(&account, &post, kind, on, result);
                }
                Msg::Sent { account, feed, client, result } => {
                    if let Some(c) = client {
                        self.clients.insert(account.clone(), c);
                    }
                    self.sent(&account, &feed, result);
                }
                Msg::Voted { account, post, client, result } => {
                    if let Some(c) = client {
                        self.clients.insert(account.clone(), c);
                    }
                    self.voted(&account, &post, result);
                }
                Msg::ScheduledList { account, client, result } => {
                    if let Some(c) = client {
                        self.clients.insert(account.clone(), c);
                    }
                    self.scheduled_arrived(&account, result);
                }
                Msg::ScheduledCancelled { account, id, client, result } => {
                    if let Some(c) = client {
                        self.clients.insert(account.clone(), c);
                    }
                    self.scheduled_cancelled(&account, &id, result);
                }
                Msg::Picked { result } => {
                    self.intake.picking = false;
                    self.intake.arrived = Some(result);
                }
                Msg::Instance { account, client, result } => {
                    if let Some(c) = client {
                        self.clients.insert(account.clone(), c);
                    }
                    self.writes.asking.remove(&account);
                    if let Ok(info) = result {
                        self.writes.info.insert(account, info);
                    }
                }
                Msg::Marked { account, client, result } => {
                    if let Some(c) = client {
                        self.clients.insert(account, c);
                    }
                    if let Err(e) = result {
                        self.messages.push(format!("Could not mark the notifications as read: {e}"));
                    }
                }
                Msg::Fetched { tab, account, feed, client, mut result } => {
                    if let Some(c) = client {
                        self.clients.insert(account.clone(), c);
                    }
                    if let Ok(p) = &mut result {
                        self.adopt_known(&account, std::iter::once(&mut **p));
                    }
                    match result {
                        Ok(post) => {
                            self.index.add(&account, &feed, std::slice::from_ref(&*post));
                            self.focus = Some(Focus { tab, account, post: *post });
                        }
                        Err(e) => self.messages.push(format!("Could not fetch the post: {e}")),
                    }
                }
            }
        }
        any
    }

    /// Forget every indexed post (Settings, SOC-040).
    pub fn clear_index(&mut self) {
        self.confirm_clear = false;
        self.index.clear();
    }

    /// Clear the index and put in again every post the feeds hold in memory (the waiting ones too).
    /// What no open feed holds is gone. Returns how many posts the index holds then (SOC-040).
    pub fn rebuild_index(&mut self) -> usize {
        self.index.clear();
        for ((account, feed), t) in &self.timelines {
            self.index.add(account, feed, &t.waiting);
            self.index.add(account, feed, &t.posts);
        }
        self.index.count(None)
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
        run(&ctx, &tx, || panic!("boom"), |m| Msg::Saved { post: "p".into(), account: "a".into(), feed: "home".into(), label: "Mastodon", result: Err(m) });
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
        /// holds a page or a save until the test lets it go
        page_gate: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
        /// tells the test that a request has arrived (one message each)
        arrived: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    }

    impl Fake {
        /// The request is here; hold it while the gate of the test is shut.
        fn hold(&self, gate: &Mutex<Option<std::sync::mpsc::Receiver<()>>>) {
            if let Some(tx) = self.arrived.lock().unwrap().as_ref() {
                let _ = tx.send(());
            }
            if let Some(rx) = gate.lock().unwrap().as_ref() {
                let _ = rx.recv_timeout(Duration::from_secs(20));
            }
        }
    }

    impl Source for Fake {
        fn post(&self, _id: &str) -> Result<Post, String> {
            Err("no such post".into())
        }
        fn thread(&self, _post: &Post) -> Result<ThreadView, String> {
            Err("no such post".into())
        }
        fn save(&self, post: &Post, _stop: &dyn Fn() -> bool) -> Option<SavedPost> {
            self.hold(&self.page_gate);
            Some(crate::social::demo::saved(post))
        }
        fn page(&self, _feed: &Feed, cursor: Option<&str>) -> Result<Page, String> {
            self.page_calls.fetch_add(1, Ordering::SeqCst);
            self.hold(&self.page_gate);
            Ok(self.pages.lock().unwrap().get(cursor.unwrap_or("")).cloned().unwrap_or_default())
        }
        fn newer(&self, feed: &Feed, since: &Since) -> Result<Fresh, String> {
            self.newer_calls.lock().unwrap().push((feed.id.clone(), since.clone()));
            self.hold(&self.gate);
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
        let at = |f: &str| FeedRef::new(e.account.id.clone(), f);
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
        st.tick(&ctx, &accounts, &[at("home"), at("home"), at(""), FeedRef::new("gone", "home")], due);
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
        st.tick(&ctx, std::slice::from_ref(&e), &[FeedRef::new(e.account.id.clone(), "home")], Instant::now() + Duration::from_secs(1000));
        // the other feed is not held back by it
        assert!(st.check(&ctx, &e, &crate::social::demo::feeds().remove(3)));
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

    /// Wait (bounded) until the pretend server has a request in hand.
    fn arrival(rx: &std::sync::mpsc::Receiver<()>) {
        rx.recv_timeout(Duration::from_secs(20)).expect("the request is on its way");
    }

    fn labels(st: &SocialState) -> Vec<String> {
        st.busy.list().into_iter().map(|t| t.label).collect()
    }

    // spec: JOB-035
    #[test]
    fn job_035_a_check_is_a_task_per_feed_that_cancel_stops_without_counting_as_a_failure_and_the_next_one_completes() {
        let (mut st, e, fake, ctx) = ready();
        let key = home_key(&e);
        let (arrived, rx) = std::sync::mpsc::channel();
        *fake.arrived.lock().unwrap() = Some(arrived);
        let (release, gate) = std::sync::mpsc::channel();
        *fake.gate.lock().unwrap() = Some(gate);
        assert!(st.check(&ctx, &e, &home_feed()));
        arrival(&rx);
        assert_eq!(labels(&st), [format!("Checking the feed of @{}…", e.account.handle)]);
        assert!(st.timelines[&key].checking);
        // a second check of the feed is refused, the other feed's is not
        assert!(!st.check(&ctx, &e, &home_feed()) && st.busy.list().len() == 1);
        // Cancel stops it; the list stays as it was and it does not count as a failure
        let task = st.busy.info(&FEED_CHECK.with(&feed_subject(&key.0, &key.1))).unwrap();
        assert!(task.cancellable && st.busy.cancel(task.id));
        release.send(()).unwrap();
        wait(&mut st, |s| !s.timelines[&key].checking);
        let t = &st.timelines[&key];
        assert!((t.failures, t.check_error.as_deref(), t.waiting.len(), t.posts.len()) == (0, None, 0, 2), "{:?}", (t.failures, &t.check_error));
        assert!(st.busy.list().is_empty());
        // the next check completes and finds the new post
        fake.newer.lock().unwrap().push(Ok(fresh(&["n1"])));
        drop(release);
        assert!(st.check(&ctx, &e, &home_feed()));
        wait(&mut st, |s| !s.timelines[&key].checking);
        assert_eq!(ids(&st.timelines[&key].waiting), ["n1"]);
        assert!(st.busy.list().is_empty());
    }

    // spec: JOB-035
    #[test]
    fn job_035_the_timer_starts_a_quiet_check_that_the_indicator_lists_only_after_two_seconds() {
        let (mut st, e, fake, ctx) = ready();
        let key = home_key(&e);
        let (arrived, rx) = std::sync::mpsc::channel();
        *fake.arrived.lock().unwrap() = Some(arrived);
        let (release, gate) = std::sync::mpsc::channel();
        *fake.gate.lock().unwrap() = Some(gate);
        st.check_every = Duration::from_secs(60);
        let t0 = Instant::now();
        st.timelines.get_mut(&key).unwrap().next_check = Some(t0);
        st.tick(&ctx, std::slice::from_ref(&e), &[FeedRef::new(e.account.id.clone(), "home")], t0 + Duration::from_secs(1));
        arrival(&rx);
        let task = FEED_CHECK.with(&feed_subject(&key.0, &key.1));
        assert!(st.busy.info(&task).is_some(), "it runs");
        assert!(st.busy.list().is_empty(), "but the indicator does not list it yet");
        assert_eq!(st.busy.list_at(Instant::now() + LISTED_AFTER).len(), 1, "after two seconds it does");
        drop(release);
        wait(&mut st, |s| !s.timelines[&key].checking);
        assert!(st.busy.list().is_empty());
    }

    // spec: JOB-035
    #[test]
    fn job_035_a_first_page_is_a_task_listed_at_once_the_next_page_after_two_seconds_and_cancel_keeps_the_list() {
        let (mut st, e, fake, ctx) = ready();
        let key = home_key(&e);
        let (arrived, rx) = std::sync::mpsc::channel();
        *fake.arrived.lock().unwrap() = Some(arrived);
        let (release, gate) = std::sync::mpsc::channel();
        *fake.page_gate.lock().unwrap() = Some(gate);
        let feed = home_feed();
        // Reload from the top: the first page
        st.reload(&ctx, &e, &feed);
        arrival(&rx);
        assert_eq!(labels(&st), [format!("Loading “{}” of @{}…", feed.title, e.account.handle)]);
        // a second start is refused
        st.reload(&ctx, &e, &feed);
        assert_eq!(st.busy.list().len(), 1);
        let task = st.busy.info(&FEED_LOAD.with(&feed_subject(&key.0, &key.1))).unwrap();
        assert!(task.cancellable && st.busy.cancel(task.id));
        release.send(()).unwrap();
        wait(&mut st, |s| !s.timelines[&key].loading);
        let t = &st.timelines[&key];
        assert_eq!((t.error.as_deref(), t.posts.len(), t.reloads), (Some("loading was stopped"), 2, 0), "the list is as it was");
        assert!(st.busy.list().is_empty());
        // Retry completes
        drop(release);
        st.reload(&ctx, &e, &feed);
        wait(&mut st, |s| !s.timelines[&key].loading);
        assert!(st.timelines[&key].error.is_none() && st.timelines[&key].loaded);
        while rx.try_recv().is_ok() {}
        // the next page follows the scrolling: running, not listed before two seconds
        let (release, gate) = std::sync::mpsc::channel();
        *fake.page_gate.lock().unwrap() = Some(gate);
        st.timelines.get_mut(&key).unwrap().cursor = Some("c2".into());
        st.load_more(&ctx, &e, &feed);
        arrival(&rx);
        assert!(st.busy.info(&FEED_LOAD.with(&feed_subject(&key.0, &key.1))).is_some_and(|t| t.label.starts_with("Loading more posts")));
        assert!(st.busy.list().is_empty() && st.busy.list_at(Instant::now() + LISTED_AFTER).len() == 1);
        drop(release);
        wait(&mut st, |s| !s.timelines[&key].loading);
    }

    // spec: JOB-035
    #[test]
    fn job_035_filling_a_gap_is_a_task_and_cancel_leaves_the_gap_for_a_later_try() {
        let (mut st, e, fake, ctx) = ready();
        let key = home_key(&e);
        let (arrived, rx) = std::sync::mpsc::channel();
        *fake.arrived.lock().unwrap() = Some(arrived);
        let (release, gate) = std::sync::mpsc::channel();
        *fake.page_gate.lock().unwrap() = Some(gate);
        let above = post_key(&st.timelines[&key].posts[0]);
        st.timelines.get_mut(&key).unwrap().gaps = vec![Gap { above: above.clone(), cursor: "c".into() }];
        let feed = home_feed();
        st.fill_gap(&ctx, &e, &feed, &above);
        arrival(&rx);
        assert_eq!(labels(&st), [format!("Loading the posts missing in “{}”…", feed.title)]);
        st.fill_gap(&ctx, &e, &feed, &above);
        assert_eq!(st.busy.list().len(), 1, "never two at once");
        let task = st.busy.info(&FEED_GAP.with(&feed_subject(&key.0, &key.1))).unwrap();
        assert!(task.cancellable && st.busy.cancel(task.id));
        release.send(()).unwrap();
        wait(&mut st, |s| !s.timelines[&key].filling);
        let t = &st.timelines[&key];
        assert_eq!((t.gaps.len(), t.check_error.as_deref()), (1, None), "the gap is still there, no error");
        assert!(st.busy.list().is_empty());
        drop(release);
        st.fill_gap(&ctx, &e, &feed, &above);
        wait(&mut st, |s| !s.timelines[&key].filling);
        assert!(st.timelines[&key].gaps.is_empty(), "the next try filled it");
    }

    // spec: JOB-036
    #[test]
    fn job_036_saving_a_post_is_a_task_that_cancel_stops_and_keeps_nothing_and_the_next_save_completes() {
        let (mut st, e, fake, ctx) = ready();
        let (arrived, rx) = std::sync::mpsc::channel();
        *fake.arrived.lock().unwrap() = Some(arrived);
        let (release, gate) = std::sync::mpsc::channel();
        *fake.page_gate.lock().unwrap() = Some(gate);
        let p = post("p1", "bob", 1, None);
        st.save(&ctx, &e, "home", &p);
        arrival(&rx);
        assert_eq!(labels(&st), ["Saving the post of @bob@m.test…"]);
        assert!(st.saving.contains(&post_key(&p)));
        // a second save of the same post is refused
        st.save(&ctx, &e, "home", &p);
        assert_eq!(st.busy.list().len(), 1);
        let task = st.busy.info(&POST_SAVE.with(&post_key(&p))).unwrap();
        assert!(task.cancellable && st.busy.cancel(task.id));
        release.send(()).unwrap();
        wait(&mut st, |s| s.saving.is_empty());
        assert_eq!(st.messages, ["Saving the post was stopped."]);
        assert!(st.saved.is_empty(), "nothing was kept for the Import dialog");
        assert!(st.busy.list().is_empty());
        // the next save completes
        drop(release);
        st.save(&ctx, &e, "home", &p);
        wait(&mut st, |s| s.saving.is_empty());
        assert_eq!(st.saved.len(), 1);
    }
}
