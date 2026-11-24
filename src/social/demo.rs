//! Made-up account and posts for screenshots (`--screenshot … --view social`) and tests: the feed
//! view can be shown and a post saved without any network. The pictures are small files made
//! in the temporary folder.

use super::note::{SavedMedia, SavedPost};
use super::*;
use std::path::PathBuf;

pub fn account() -> SocialAccountEntry {
    SocialAccountEntry { account: Account::new(Network::Mastodon, "https://demo.social", "ada@demo.social", "Ada Demo"), secret: Default::default() }
}

pub fn feeds() -> Vec<Feed> {
    let mut out = vec![Feed { id: "home".into(), title: "Home".into() }];
    out.extend(super::notifications::extra_feeds());
    out.push(Feed { id: "list:1".into(), title: "List: Rust people".into() });
    out
}

/// A picture file in the demo folder (made on first use): a colour gradient with a few shapes.
fn picture(name: &str, w: u32, h: u32, hue: f32) -> String {
    let dir = std::env::temp_dir().join("shale-social-demo");
    let path: PathBuf = dir.join(name);
    if !path.is_file() {
        let _ = std::fs::create_dir_all(&dir);
        let img = image::RgbaImage::from_fn(w, h, |x, y| {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let wave = ((fx * 9.0 + hue).sin() * 0.5 + 0.5) * 0.35;
            let c = |k: f32| ((0.25 + 0.55 * (fy * (1.0 - k) + fx * k) + wave * k) * 255.0).clamp(0.0, 255.0) as u8;
            let dx = fx - 0.5 - (hue.sin() * 0.2);
            let dy = fy - 0.55;
            if dx * dx * 2.2 + dy * dy * 6.0 < 0.05 {
                image::Rgba([250, 232, 190, 255])
            } else {
                image::Rgba([c(0.2 + hue.fract() * 0.3), c(0.5), c(0.9 - hue.fract() * 0.3), 255])
            }
        });
        let _ = img.save(&path);
    }
    path.to_string_lossy().into_owned()
}

fn author(handle: &str, name: &str, hue: f32) -> Author {
    Author {
        id: format!("acct-{handle}"),
        handle: handle.into(),
        display_name: name.into(),
        profile_url: format!("https://demo.social/@{}", handle.split('@').next().unwrap_or("")),
        avatar_url: picture(&format!("avatar-{}.png", (hue * 10.0) as u32), 96, 96, hue),
    }
}

fn post(id: &str, who: Author, ago_minutes: i64, text: &str) -> Post {
    Post {
        network: Network::Mastodon,
        id: id.into(),
        cid: String::new(),
        url: format!("https://demo.social/@{}/{id}", who.handle.split('@').next().unwrap_or("")),
        author: who,
        created_at: crate::calendar::time::now() - ago_minutes * 60,
        text_markdown: text.into(),
        content_warning: None,
        media: vec![],
        quote: None,
        link_card: None,
        reply_to: None,
        reposted_by: None,
        reply_count: 0,
        boost_count: 0,
        like_count: 0,
        mine: Mine::default(),
        reply_root: None,
        visibility: Some("public".into()),
        poll: None,
        notice: None,
    }
}

fn image_media(name: &str, w: u32, h: u32, hue: f32, alt: &str) -> Media {
    let path = picture(name, w, h, hue);
    Media { kind: MediaKind::Image, url: path.clone(), preview_url: path, alt: alt.into(), file_name_hint: name.into(), blob_cid: None }
}

pub fn thread_id() -> &'static str {
    "demo-thread-1"
}

/// The three posts of the demo thread, in reading order.
pub fn thread() -> Vec<Post> {
    let grace = author("grace@fosstodon.org", "Grace Hopper", 1.3);
    let mut a = post(thread_id(), grace.clone(), 95, "A short thread on why plain text files last longer than apps (1/3)\n\nEvery app I loved is gone. The notes I wrote in plain files are still here.");
    a.reply_count = 2;
    a.like_count = 41;
    a.boost_count = 12;
    let mut b = post("demo-thread-2", grace.clone(), 94, "2/3 Formats with one reader die when the reader dies. Markdown has hundreds of readers, and you can read it with your eyes.");
    b.reply_to = Some(thread_id().into());
    let mut c = post("demo-thread-3", grace, 93, "3/3 So: keep the files, and let the tools come and go. Here is the folder I mean:");
    c.reply_to = Some("demo-thread-2".into());
    c.media = vec![image_media("demo-folder.png", 600, 360, 2.1, "A folder with many small text files")];
    vec![a, b, c]
}

/// A few posts: a plain one with links, a boost, a content warning, pictures, a thread, a quote.
pub fn posts() -> Vec<Post> {
    let ada = author("ada@demo.social", "Ada Demo", 0.4);
    let linus = author("linus@kernel.social", "Linus T.", 3.0);
    let mut p1 = post(
        "demo-1",
        author("alan@mathstodon.xyz", "Alan Turing", 0.9),
        8,
        "Morning! Finished reading [@grace](https://fosstodon.org/@grace)'s notes on [#plaintext](https://demo.social/tags/plaintext). Worth your time: https://example.org/plain-text-forever",
    );
    p1.like_count = 17;
    p1.mine.liked = true;
    p1.boost_count = 3;
    p1.reply_count = 2;
    let mut p2 = post("demo-2", linus, 25, "Merge window is open. Please send patches that compile.");
    p2.reposted_by = Some(ada);
    p2.like_count = 320;
    p2.boost_count = 88;
    p2.mine.boosted = true;
    let mut p3 = post("demo-3", author("ken@demo.social", "Ken Thompson", 2.4), 41, "I would rather not say this out loud, but the weather forecast was right for once.");
    p3.content_warning = Some("Weather, mild sarcasm".into());
    let mut p4 = post("demo-4", author("margaret@space.example", "Margaret H.", 1.8), 62, "Sunrise over the lab this morning. Two shots, same hill.");
    p4.media = vec![image_media("demo-sunrise-1.png", 800, 500, 0.3, "A hill with a bright sun in an orange sky"), image_media("demo-sunrise-2.png", 800, 500, 1.1, "The same hill a minute later, more purple")];
    p4.like_count = 95;
    let mut p5 = post("demo-5", author("grace@fosstodon.org", "Grace Hopper", 1.3), 95, "Poll: what do you keep your notes in?");
    p5.poll = Some(super::poll::Poll {
        id: "demo-poll-1".into(),
        options: [("Plain files in a folder", 41), ("An app with sync", 22), ("Paper", 9)].iter().map(|(t, v)| super::poll::PollOption { title: (*t).into(), votes: Some(*v) }).collect(),
        expires_at: Some(crate::calendar::time::now() + 20 * 3600),
        expired: false,
        multiple: false,
        votes_count: 72,
        voters_count: Some(72),
        voted: true,
        own_votes: vec![0],
    });
    let mut p6 = post("demo-6", author("edsger@demo.social", "Edsger D.", 2.9), 130, "Simplicity is prerequisite for reliability.");
    let mut quoted = post("demo-6q", author("tony@demo.social", "Tony Hoare", 0.7), 400, "Null references were a billion dollar mistake. Sorry about that.");
    quoted.like_count = 1200;
    p6.quote = Some(Box::new(quoted));
    p6.link_card = Some(LinkCard { url: "https://example.org/reliability".into(), title: "On the reliability of programs".into(), description: "An essay about keeping things simple enough to understand.".into() });
    let mut out = vec![p1, p2, p3, p4];
    out.push(thread().remove(0));
    out.push(p6);
    out.push(p5);
    out
}

/// Three posts that arrive while the demo runs, newest first (SOC-027).
pub fn new_posts() -> Vec<Post> {
    let grace = author("grace@fosstodon.org", "Grace Hopper", 1.3);
    let ada = author("ada@demo.social", "Ada Demo", 0.4);
    let mut a = post("demo-new-3", grace, 1, "Just now: the notes app does not need a server, a login or a subscription. It needs a folder.");
    a.like_count = 2;
    let b = post("demo-new-2", ada, 3, "Reading in the train. Back in the evening.");
    let c = post("demo-new-1", author("alan@mathstodon.xyz", "Alan Turing", 0.9), 5, "Does anyone have a good proof that this list is not infinite?");
    vec![a, b, c]
}

/// The account's own posts ("Your posts", SOC-074), newest first.
pub fn own_posts() -> Vec<Post> {
    let ada = author("ada@demo.social", "Ada Demo", 0.4);
    let mut a = post("demo-own-1", ada.clone(), 20, "Plain text files outlive every app I have ever used. Mine are older than my phone.");
    (a.like_count, a.boost_count, a.reply_count) = (9, 2, 1);
    let mut b = post("demo-own-2", ada.clone(), 190, "Reading in the train. Back in the evening.");
    b.like_count = 3;
    let mut c = post("demo-own-3", ada, 1300, "Does anyone else keep a folder called \"later\" that is older than their last job?");
    c.reply_to = Some("demo-thread-1".into());
    vec![a, b, c]
}

/// The Notifications feed of the demo account (SOC-078), newest first: five people favourited
/// the first post (one entry), a reply, a boost, a follow, a mention and an older favourite that
/// was read. The server's marker is 895: the ones above it are unread.
pub fn notifications() -> Vec<Post> {
    use super::notifications::{entries, Kind, Raw};
    let now = crate::calendar::time::now();
    let own = own_posts();
    let people = [
        author("alan@mathstodon.xyz", "Alan Turing", 0.9),
        author("grace@fosstodon.org", "Grace Hopper", 1.3),
        author("linus@kernel.social", "Linus T.", 3.0),
        author("ken@demo.social", "Ken Thompson", 2.4),
        author("margaret@space.example", "Margaret H.", 1.8),
        author("tony@demo.social", "Tony Hoare", 0.7),
    ];
    let raw = |id: i64, kind: Kind, who: usize, minutes: i64, post: Option<Post>| Raw { id: id.to_string(), mark: id.to_string(), rank: id, kind, who: people[who].clone(), at: now - minutes * 60, unread: id > 895, post };
    let mut reply = post("demo-reply-n1", people[1].clone(), 12, "@ada Same here. Which editor do you open them with?");
    reply.reply_to = Some(own[0].id.clone());
    let mut mention = post("demo-mention-n1", people[0].clone(), 40, "Notes on notes: [@ada](https://demo.social/@ada) has the right idea about plain files.");
    mention.like_count = 4;
    let raws = vec![
        raw(910, Kind::Reply, 1, 12, Some(reply)),
        raw(909, Kind::Like, 0, 18, Some(own[0].clone())),
        raw(908, Kind::Like, 2, 19, Some(own[0].clone())),
        raw(907, Kind::Like, 3, 26, Some(own[0].clone())),
        raw(906, Kind::Like, 4, 31, Some(own[0].clone())),
        raw(905, Kind::Like, 5, 33, Some(own[0].clone())),
        raw(904, Kind::Follow, 5, 35, None),
        raw(903, Kind::Mention, 0, 40, Some(mention)),
        raw(902, Kind::Boost, 4, 150, Some(own[1].clone())),
        raw(890, Kind::Like, 2, 480, Some(own[1].clone())),
    ];
    entries(Network::Mastodon, raws)
}

/// A pretend server for the demo: nothing older than the posts shown, and the first check finds
/// the posts of `new_posts`.
pub struct DemoSource;

impl super::state::Source for DemoSource {
    fn page(&self, feed: &Feed, _cursor: Option<&str>) -> Result<Page, String> {
        Ok(match feed.id.as_str() {
            super::notifications::NOTIFICATIONS => Page { posts: notifications(), ..Default::default() },
            super::notifications::OWN => Page { posts: own_posts(), ..Default::default() },
            _ => Page::default(),
        })
    }
    fn post(&self, id: &str) -> Result<Post, String> {
        posts().into_iter().chain(thread()).chain(new_posts()).find(|p| p.id == id).ok_or_else(|| "no such post".to_string())
    }
    fn thread(&self, post: &Post) -> Result<ThreadView, String> {
        // the demo thread and the replies to it are one conversation
        let everything: Vec<Post> = thread().into_iter().chain(replies().into_iter().map(|r| r.post)).collect();
        let mut ancestors = vec![];
        let mut up = post.reply_to.clone();
        while let Some(p) = up.and_then(|id| everything.iter().find(|p| p.id == id)) {
            ancestors.push(p.clone());
            up = p.reply_to.clone();
        }
        ancestors.reverse();
        let mut below: Vec<Post> = vec![];
        let mut ids = vec![post.id.clone()];
        while let Some(next) = everything.iter().find(|p| p.reply_to.as_ref().is_some_and(|r| ids.contains(r)) && !ids.contains(&p.id)) {
            ids.push(next.id.clone());
            below.push(next.clone());
        }
        Ok(super::thread_view_of(post, ancestors, &below))
    }
    fn newer(&self, feed: &Feed, since: &Since) -> Result<Fresh, String> {
        if feed.id != "home" {
            return Ok(Fresh::default());
        }
        Ok(Fresh { posts: new_posts().into_iter().filter(|p| !since.known.contains(&super::state::post_key(p))).collect(), ..Default::default() })
    }
}

/// Replies of other people to the demo thread (a small tree).
pub fn replies() -> Vec<Reply> {
    let mut a = post("demo-reply-1", author("alan@mathstodon.xyz", "Alan Turing", 0.9), 80, "Plain files, yes. I would add: write the *why* next to the *what*.");
    a.reply_to = Some("demo-thread-2".into());
    let mut b = post("demo-reply-2", author("grace@fosstodon.org", "Grace Hopper", 1.3), 70, "Good point, I do that in my notes.");
    b.reply_to = Some("demo-reply-1".into());
    let mut c = post("demo-reply-3", author("ken@demo.social", "Ken Thompson", 2.4), 60, "Same here. Plain text and grep.");
    c.reply_to = Some("demo-thread-3".into());
    vec![Reply { post: a, depth: 0 }, Reply { post: b, depth: 1 }, Reply { post: c, depth: 0 }]
}

/// What `Client::save` would fetch for a demo post: its thread and its pictures (read from the files).
pub fn saved(p: &Post) -> SavedPost {
    let thread = if p.id == thread_id() { thread() } else { vec![Post { reposted_by: None, ..p.clone() }] };
    let mut media = vec![];
    for (pi, t) in thread.iter().enumerate() {
        for (mi, m) in t.media.iter().enumerate() {
            if let Ok(data) = crate::fsutil::read(&m.url) {
                media.push(SavedMedia { file_name: format!("{}-{}.png", t.author.handle.replace('@', "_"), media.len() + 1), data, post: pi, item: mi });
            }
        }
    }
    let replies = if p.id == thread_id() { replies() } else { vec![] };
    SavedPost { thread, reposted_by: p.reposted_by.clone(), media, failed_media: vec![], thread_error: None, replies, root: None }
}
