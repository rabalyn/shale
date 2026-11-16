//! From a post to a note: `fetch_saved` gets the whole self-thread and every picture / video
//! (background job, blocking), `post_note` writes the Markdown (pure, no network), `post_title`
//! names the note, `marker` / `note_has_post` spot a second import of the same post.
//!
//! How the pieces fit with the vault (see `import.rs` for e-mails): the caller saves each
//! `SavedPost::media` file with `media::save_bytes(vault, &m.file_name, &m.data, date)`, collects the
//! returned paths in the same order, and passes them to `post_note`.

use super::markup::{md_link, md_url, plain_text};
use super::*;
use crate::calendar::time as t;
use crate::import::{front, local_stamp, safe_title, yaml};
use crate::media;
use std::path::{Path, PathBuf};

/// One downloaded media file.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedMedia {
    /// the name to save it under (with extension)
    pub file_name: String,
    pub data: Vec<u8>,
    /// which post of `SavedPost::thread` it belongs to, and which of that post's `media`
    pub post: usize,
    pub item: usize,
}

/// A media file that could not be downloaded (too big, gone, error): the note links to its address.
#[derive(Clone, Debug, PartialEq)]
pub struct FailedMedia {
    pub url: String,
    pub error: String,
    pub post: usize,
    pub item: usize,
}

/// Everything needed to write the note, fetched in one go.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedPost {
    /// the self-thread in reading order (just the post when it is not a thread)
    pub thread: Vec<Post>,
    /// who boosted / reposted the post in the feed
    pub reposted_by: Option<Author>,
    pub media: Vec<SavedMedia>,
    pub failed_media: Vec<FailedMedia>,
    /// why only the one post was saved, when the thread could not be fetched
    pub thread_error: Option<String>,
    /// replies of other people below the thread (their media is not downloaded, only linked);
    /// they go into the note only when they are still here (the Import dialog can drop them)
    pub replies: Vec<Reply>,
}

/// Fetch the self-thread of `post` and download all media of all its posts (blocking: use a
/// background thread). Problems with single files or the thread are not fatal, see the fields.
pub fn fetch_saved(client: &mut Client, post: &Post) -> SavedPost {
    let (mut thread, thread_error, replies) = match client.thread_with_replies(post) {
        Ok((t, r)) if t.iter().any(|p| p.id == post.id) => (t, None, r),
        Ok(_) => (vec![post.clone()], None, vec![]),
        Err(e) => (vec![post.clone()], Some(e), vec![]),
    };
    // the post as the feed showed it (same content; keeps the card and counters the user saw)
    if thread.len() == 1 {
        thread[0] = Post { reposted_by: None, ..post.clone() };
    }
    let mut saved = SavedPost { thread, reposted_by: post.reposted_by.clone(), media: vec![], failed_media: vec![], thread_error, replies };
    let mut n = 0;
    for (pi, p) in saved.thread.clone().iter().enumerate() {
        for (mi, m) in p.media.iter().enumerate() {
            n += 1;
            match client.download_media(p, m) {
                Ok((data, content_type)) => saved.media.push(SavedMedia { file_name: media_file_name(p, m, n, content_type.as_deref()), data, post: pi, item: mi }),
                Err(error) => saved.failed_media.push(FailedMedia { url: m.url.clone(), error, post: pi, item: mi }),
            }
        }
    }
    saved
}

/// "ada_m.test-1.jpg": author and running number, the extension from the address or the content type.
fn media_file_name(p: &Post, m: &Media, n: usize, content_type: Option<&str>) -> String {
    let stem: String = p.author.handle.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '.') { c } else { '_' }).collect();
    let ext = Path::new(&m.file_name_hint).extension().and_then(|e| e.to_str()).filter(|e| !e.is_empty() && e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric())).map(|e| e.to_ascii_lowercase()).or_else(|| {
        content_type.and_then(|c| {
            match c {
                "image/jpeg" => "jpg",
                "image/png" => "png",
                "image/webp" => "webp",
                "image/gif" => "gif",
                "video/mp4" => "mp4",
                "video/webm" => "webm",
                "audio/mpeg" => "mp3",
                "audio/ogg" => "ogg",
                _ => return None,
            }
            .to_string()
            .into()
        })
    });
    format!("{}-{n}.{}", stem.trim_matches('.'), ext.unwrap_or_else(|| "bin".into()))
}

/// The date of a post in the local time zone (today when the post has no date).
fn local_day(at: i64) -> (i32, u32, u32) {
    if at == 0 {
        media::local_date()
    } else {
        t::civil_from_days(t::utc_to_local(at).0)
    }
}

/// The folder-date for saving media: the date of the first post.
pub fn media_date(saved: &SavedPost) -> (i32, u32, u32) {
    local_day(saved.thread.first().map(|p| p.created_at).unwrap_or(0))
}

/// "2024-05-17 @ada.bsky.social – first words of the post": the note's name, safe for file names.
pub fn post_title(p: &Post) -> String {
    let (y, m, d) = local_day(p.created_at);
    let plain = plain_text(&p.text_markdown);
    let mut words = String::new();
    for w in plain.split_whitespace() {
        if words.chars().count() + w.chars().count() > 60 || words.split(' ').count() >= 9 {
            break;
        }
        if !words.is_empty() {
            words.push(' ');
        }
        words.push_str(w);
    }
    let words = if words.is_empty() { plain.chars().take(40).collect() } else { words };
    let words = if words.trim().is_empty() {
        if p.media.is_empty() {
            "(post)".to_string()
        } else {
            "(media)".to_string()
        }
    } else {
        words
    };
    let raw = format!("{y:04}-{m:02}-{d:02} @{} – {}", p.author.handle, words);
    let cleaned: String = raw.chars().map(|c| if matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { ' ' } else { c }).filter(|c| !c.is_control()).collect();
    safe_title(&cleaned).trim_end_matches(['.', ' ']).to_string()
}

/// The front matter line that marks a note as made from this post (to spot a second import).
pub fn marker(post_id: &str) -> String {
    format!("post-id: {}", yaml(post_id))
}

/// Whether a note's text says it was made from this post (as the first post of its thread, or
/// as any other post of the thread): look at the first lines.
pub fn note_has_post(note_text: &str, post_id: &str) -> bool {
    let m = marker(post_id);
    let quoted = yaml(post_id);
    note_text.lines().take(40).any(|l| {
        let l = l.trim();
        l == m || (l.starts_with("thread-ids:") && l.contains(&quoted))
    })
}

fn kind_word(k: MediaKind) -> &'static str {
    match k {
        MediaKind::Image => "image",
        MediaKind::Video => "video",
        MediaKind::Gifv => "animation",
        MediaKind::Audio => "audio",
        MediaKind::Other => "media",
    }
}

/// `[image](address)`, or just the address if it cannot be a Markdown link.
fn plain_link(label: &str, url: &str) -> String {
    match md_url(url) {
        Some(u) => format!("[{label}]({u})"),
        None => label.to_string(),
    }
}

fn quote_lines(s: &str) -> String {
    s.lines().map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") }).collect::<Vec<_>>().join("\n")
}

/// The Markdown of one post of a note (without its header).
fn post_body(saved: &SavedPost, pi: usize, rel: &[PathBuf]) -> String {
    let p = &saved.thread[pi];
    let mut blocks: Vec<String> = vec![];
    if let Some(cw) = &p.content_warning {
        blocks.push(quote_lines(&format!("CW: {cw}")));
    }
    if !p.text_markdown.trim().is_empty() {
        blocks.push(p.text_markdown.trim().to_string());
    }
    for (mi, m) in p.media.iter().enumerate() {
        let mut line = match saved.media.iter().position(|s| s.post == pi && s.item == mi).and_then(|k| rel.get(k)) {
            Some(path) => media::link_for(path),
            None => plain_link(kind_word(m.kind), &m.url),
        };
        if !m.alt.trim().is_empty() {
            line.push_str(&format!("\n*{}*", m.alt.trim().replace('\n', " ").replace('*', "\\*")));
        }
        blocks.push(line);
    }
    if let Some(q) = &p.quote {
        let mut inner = vec![format!("**Quote:** {} · {}", md_link(&format!("{} (@{})", q.author.name(), q.author.handle), &q.author.profile_url), plain_link("original", &q.url))];
        if let Some(cw) = &q.content_warning {
            inner.push(format!("CW: {cw}"));
        }
        if !q.text_markdown.trim().is_empty() {
            inner.push(q.text_markdown.trim().to_string());
        }
        for m in &q.media {
            inner.push(if m.alt.trim().is_empty() { plain_link(kind_word(m.kind), &m.url) } else { format!("{} — {}", plain_link(kind_word(m.kind), &m.url), m.alt.trim().replace('\n', " ")) });
        }
        blocks.push(quote_lines(&inner.join("\n")));
    }
    if let Some(c) = &p.link_card {
        let title = if c.title.trim().is_empty() { c.url.as_str() } else { c.title.trim() };
        let d = c.description.trim().replace('\n', " ");
        blocks.push(format!("{}{}", md_link(title, &c.url), if d.is_empty() { String::new() } else { format!(" — {d}") }));
    }
    blocks.join("\n\n")
}

/// "## Replies": each reply as a header line and its text inside nested block quotes (one level
/// per depth, at most 4); pictures and videos are only linked. Empty without replies.
fn replies_section(saved: &SavedPost) -> String {
    if saved.replies.is_empty() {
        return String::new();
    }
    let mut s = "\n## Replies\n".to_string();
    for r in &saved.replies {
        let p = &r.post;
        let mut head = vec![format!("**{}**", md_link(&format!("{} (@{})", p.author.name(), p.author.handle), &p.author.profile_url))];
        if p.created_at != 0 {
            head.push(local_stamp(p.created_at, false));
        }
        if !p.url.is_empty() {
            head.push(md_link("link", &p.url));
        }
        let mut lines = vec![head.join(" · "), String::new()];
        if let Some(cw) = &p.content_warning {
            lines.push(format!("CW: {cw}"));
            lines.push(String::new());
        }
        if !p.text_markdown.trim().is_empty() {
            lines.extend(p.text_markdown.trim().lines().map(String::from));
        }
        for m in &p.media {
            lines.push(if m.alt.trim().is_empty() { plain_link(kind_word(m.kind), &m.url) } else { format!("{} — {}", plain_link(kind_word(m.kind), &m.url), m.alt.trim().replace('\n', " ")) });
        }
        let prefix = ">".repeat(r.depth.min(3) + 1);
        s.push('\n');
        for l in lines {
            s.push_str(&if l.is_empty() { format!("{prefix}\n") } else { format!("{prefix} {l}\n") });
        }
    }
    s
}

/// The note for a saved post or thread. `media_rel_paths`: where each of `saved.media` was saved
/// in the vault (same order). `tags` and `people_links` (e.g. `[[Ada Lovelace]]`) as for e-mails.
pub fn post_note(saved: &SavedPost, media_rel_paths: &[PathBuf], tags: &[String], people_links: &[String]) -> String {
    let first = &saved.thread[0];
    let n = saved.thread.len();
    let when = |p: &Post| if p.created_at == 0 { String::new() } else { local_stamp(p.created_at, false) };
    let name = first.author.name();
    let mut s = front(
        &[
            ("type", "social-post".into()),
            ("network", first.network.key().into()),
            ("author", yaml(&format!("@{}", first.author.handle))),
            ("author-name", if first.author.display_name.trim().is_empty() { String::new() } else { yaml(&first.author.display_name) }),
            ("date", when(first)),
            ("url", yaml(&first.url)),
            ("post-id", yaml(&first.id)),
            ("thread", if n > 1 { n.to_string() } else { String::new() }),
            ("replies", if saved.replies.is_empty() { String::new() } else { saved.replies.len().to_string() }),
            ("thread-ids", if n > 1 { format!("[{}]", saved.thread.iter().map(|p| yaml(&p.id)).collect::<Vec<_>>().join(", ")) } else { String::new() }),
        ],
        tags,
    );
    s.push_str(&format!("\n# {}\n\n", post_title(first)));
    s.push_str(&format!("- **Author:** {}\n", md_link(&format!("{name} (@{})", first.author.handle), &first.author.profile_url)));
    if first.created_at != 0 {
        s.push_str(&format!("- **Posted:** {}\n", when(first)));
    }
    s.push_str(&format!("- **Source:** {}\n", if first.url.is_empty() { String::new() } else { md_link(&first.url, &first.url) }));
    if n > 1 {
        s.push_str(&format!("- **Thread:** {n} posts by @{}\n", first.author.handle));
    }
    if let Some(b) = &saved.reposted_by {
        s.push_str(&format!("- **{} by:** {}\n", if first.network == Network::Mastodon { "Boosted" } else { "Reposted" }, md_link(&format!("{} (@{})", b.name(), b.handle), &b.profile_url)));
    }
    if !people_links.is_empty() {
        s.push_str(&format!("- **People:** {}\n", people_links.join(", ")));
    }
    if n == 1 {
        s.push_str("\n## Post\n\n");
        s.push_str(&post_body(saved, 0, media_rel_paths));
        s.push('\n');
    } else {
        s.push_str("\n## Thread\n");
        for (i, p) in saved.thread.iter().enumerate() {
            if i > 0 {
                s.push_str("\n---\n");
            }
            let mut head = vec![format!("**{}/{n}**", i + 1)];
            if p.created_at != 0 {
                head.push(when(p));
            }
            if !p.url.is_empty() {
                head.push(md_link("link", &p.url));
            }
            s.push_str(&format!("\n{}\n\n{}\n", head.join(" · "), post_body(saved, i, media_rel_paths)));
        }
    }
    s.push_str(&replies_section(saved));
    if !saved.failed_media.is_empty() || saved.thread_error.is_some() {
        // the note says what is missing instead of silently lacking it
        s.push_str("\n> [!warning] Not everything could be saved\n");
        if let Some(e) = &saved.thread_error {
            s.push_str(&format!("> Only this post was saved, the rest of the thread could not be fetched ({}).\n", e.replace('\n', " ")));
        }
        if !saved.failed_media.is_empty() {
            s.push_str("> Pictures or videos that could not be downloaded are linked to their address on the server.\n");
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::super::tests::post;
    use super::*;

    fn saved_of(thread: Vec<Post>) -> SavedPost {
        SavedPost { thread, reposted_by: None, media: vec![], failed_media: vec![], thread_error: None, replies: vec![] }
    }

    fn media(kind: MediaKind, url: &str, alt: &str) -> Media {
        Media { kind, url: url.into(), preview_url: String::new(), alt: alt.into(), file_name_hint: file_name_from_url(url, "x.jpg"), blob_cid: None }
    }

    #[test]
    fn titles_are_short_and_safe() {
        let mut p = post("1", "ada", 1_715_941_800, None);
        p.author.handle = "ada@m.test".into();
        p.text_markdown = "Hello [@bob](https://m.test/@bob), see https://example.com/x: a/b \"quoted\" #1? and many more words to cut here and there".into();
        let t = post_title(&p);
        assert!(t.starts_with("20") && t.contains(" @ada@m.test – Hello @bob, see a b quoted 1 and"), "{t}");
        assert!(!t.contains(['/', ':', '"', '?', '#', '[']) || t.contains("@ada@m.test"), "{t}");
        assert!(!t.contains('/') && !t.contains('"') && !t.contains('?') && !t.contains('['), "{t}");
        assert!(t.chars().count() < 110, "{t}");
        p.text_markdown = String::new();
        assert!(post_title(&p).ends_with("– (post)"));
        p.media.push(media(MediaKind::Image, "https://f.test/a.png", ""));
        assert!(post_title(&p).ends_with("– (media)"));
        p.text_markdown = "Grüße 😀 aus Köln".into();
        assert!(post_title(&p).ends_with("– Grüße 😀 aus Köln"));
    }

    #[test]
    fn single_post_note() {
        let mut p = post("42", "ada", 1_715_941_800, None);
        p.author.handle = "ada@m.test".into();
        p.author.display_name = "Ada \"Countess\" L.".into();
        p.content_warning = Some("food".into());
        p.text_markdown = "Lunch [#yum](https://m.test/tags/yum)\n\nsecond paragraph".into();
        p.media = vec![media(MediaKind::Image, "https://f.test/a.png", "a bowl of soup"), media(MediaKind::Video, "https://f.test/v.mp4", ""), media(MediaKind::Image, "https://f.test/gone.jpg", "")];
        p.link_card = Some(LinkCard { url: "https://example.com/r".into(), title: "Recipe".into(), description: "Soup".into() });
        let mut q = post("7", "bob", 1_715_900_000, None);
        q.text_markdown = "orig\n\nmore".into();
        q.media = vec![media(MediaKind::Image, "https://f.test/q.png", "q alt")];
        p.quote = Some(Box::new(q));
        let mut saved = saved_of(vec![p]);
        saved.reposted_by = Some(Author { handle: "cy@m.test".into(), display_name: "Cy".into(), profile_url: "https://m.test/@cy".into(), ..Default::default() });
        saved.media = vec![SavedMedia { file_name: "a.png".into(), data: vec![1], post: 0, item: 0 }, SavedMedia { file_name: "v.mp4".into(), data: vec![2], post: 0, item: 1 }];
        saved.failed_media = vec![FailedMedia { url: "https://f.test/gone.jpg".into(), error: "404".into(), post: 0, item: 2 }];
        let paths = vec![PathBuf::from("z_media/2024/05/240517_ada-1.png"), PathBuf::from("z_media/2024/05/240517_ada-2.mp4")];
        let n = post_note(&saved, &paths, &["social".into(), "recipes".into()], &["[[Ada]]".into()]);
        let (y, m, d) = local_day(1_715_941_800);
        let stamp = local_stamp(1_715_941_800, false);
        assert!(n.starts_with("---\ntype: social-post\nnetwork: mastodon\nauthor: \"@ada@m.test\"\nauthor-name: \"Ada \\\"Countess\\\" L.\"\n"), "{n}");
        for line in
            [format!("date: {stamp}"), "url: \"https://m.test/@ada/42\"".into(), "post-id: \"42\"".into(), "tags: [social, recipes]".into(), format!("# {y:04}-{m:02}-{d:02} @ada@m.test – Lunch #yum second paragraph").replace("#yum", "yum")]
        {
            let l = line.trim_start_matches("# ");
            assert!(n.contains(l) || n.contains(&line), "{l}\n{n}");
        }
        assert!(!n.contains("thread:") && !n.contains("thread-ids"), "{n}");
        for line in [
            "- **Author:** [Ada \"Countess\" L. (@ada@m.test)](https://m.test/@ada)",
            &format!("- **Posted:** {stamp}"),
            "- **Source:** [https://m.test/@ada/42](https://m.test/@ada/42)",
            "- **Boosted by:** [Cy (@cy@m.test)](https://m.test/@cy)",
            "- **People:** [[Ada]]",
            "## Post",
            "> CW: food",
            "Lunch [#yum](https://m.test/tags/yum)\n\nsecond paragraph",
            "![[z_media/2024/05/240517_ada-1.png]]\n*a bowl of soup*",
            "![[z_media/2024/05/240517_ada-2.mp4]]",
            "[image](https://f.test/gone.jpg)",
            "> **Quote:** [BOB (@bob@m.test)](https://m.test/@bob) · [original](https://m.test/@bob/7)\n> orig\n>\n> more\n> [image](https://f.test/q.png) — q alt",
            "[Recipe](https://example.com/r) — Soup",
            "> [!warning]",
        ] {
            assert!(n.contains(line), "missing {line:?} in\n{n}");
        }
        // the marker is findable, also from a line-by-line scan
        assert!(note_has_post(&n, "42") && !note_has_post(&n, "4"));
    }

    #[test]
    fn thread_note_layout() {
        let mut ps: Vec<Post> = (1..=3).map(|i| post(&format!("p{i}"), "ada", 1_715_941_800 + i * 60, None)).collect();
        ps[1].media = vec![media(MediaKind::Image, "https://f.test/b.jpg", "")];
        for p in &mut ps {
            p.network = Network::Bluesky;
        }
        let mut saved = saved_of(ps);
        saved.media = vec![SavedMedia { file_name: "b.jpg".into(), data: vec![], post: 1, item: 0 }];
        saved.reposted_by = Some(Author { handle: "cy.test".into(), ..Default::default() });
        let n = post_note(&saved, &[PathBuf::from("z_media/b.jpg")], &[], &[]);
        assert!(n.contains("network: bluesky\n") && n.contains("thread: 3\n") && n.contains("thread-ids: [\"p1\", \"p2\", \"p3\"]\n"), "{n}");
        assert!(n.contains("- **Thread:** 3 posts by @ada@m.test\n") && n.contains("- **Reposted by:** cy.test (@cy.test)\n"), "{n}");
        assert!(n.contains("## Thread\n\n**1/3** · ") && n.contains("\n---\n\n**2/3** · ") && n.contains("\n---\n\n**3/3** · "), "{n}");
        assert!(n.contains("text of p2\n\n![[z_media/b.jpg]]\n"), "{n}");
        assert!(!n.contains("## Post"));
        let a = n.find("text of p1").unwrap();
        let b = n.find("text of p2").unwrap();
        let c = n.find("text of p3").unwrap();
        assert!(a < b && b < c);
        // importing a second time: found by the first post and by any other post of the thread
        assert!(note_has_post(&n, "p1") && note_has_post(&n, "p3") && !note_has_post(&n, "p"));
        assert_eq!(marker("p1"), "post-id: \"p1\"");
    }

    #[test]
    fn failed_thread_and_missing_paths_fall_back_to_links() {
        let mut p = post("1", "ada", 0, None);
        p.text_markdown = "x".into();
        p.media = vec![media(MediaKind::Video, "https://f.test/v.mp4", "clip")];
        let mut saved = saved_of(vec![p]);
        saved.thread_error = Some("offline".into());
        saved.failed_media = vec![FailedMedia { url: "https://f.test/v.mp4".into(), error: "too big".into(), post: 0, item: 0 }];
        let n = post_note(&saved, &[], &[], &[]);
        assert!(n.contains("[video](https://f.test/v.mp4)\n*clip*") && n.contains("offline") && !n.contains("- **Posted:**") && !n.contains("\ndate:"), "{n}");
        // a saved file without a path (caller lost it) is a link too, never a panic
        saved.failed_media.clear();
        saved.media = vec![SavedMedia { file_name: "v.mp4".into(), data: vec![], post: 0, item: 0 }];
        assert!(post_note(&saved, &[], &[], &[]).contains("[video](https://f.test/v.mp4)"));
    }

    // spec: SOC-020
    #[test]
    fn replies_are_a_quoted_section_only_when_there_are_some() {
        let mut saved = saved_of(vec![post("p1", "ada", 1_715_941_800, None)]);
        let plain = post_note(&saved, &[], &[], &[]);
        assert!(!plain.contains("## Replies") && !plain.contains("replies:"), "{plain}");
        let mut r1 = post("c1", "cy", 1_715_941_900, Some("p1"));
        r1.media = vec![media(MediaKind::Image, "https://f.test/pic.jpg", "a cat")];
        let r2 = post("c2", "dee", 1_715_942_000, Some("c1"));
        let deep = post("c3", "eve", 1_715_942_100, Some("c2"));
        saved.replies = vec![Reply { post: r1, depth: 0 }, Reply { post: r2, depth: 1 }, Reply { post: deep, depth: 9 }];
        let n = post_note(&saved, &[], &[], &[]);
        assert!(n.contains("replies: 3\n") && n.contains("\n## Replies\n"), "{n}");
        assert!(n.contains("\n> **[CY (@cy@m.test)](https://m.test/@cy)** · ") && n.contains(" · [link](https://m.test/@cy/c1)\n>\n> text of c1\n> [image](https://f.test/pic.jpg) — a cat\n"), "{n}");
        assert!(n.contains("\n>> **[DEE") && n.contains("\n>> text of c2\n"), "{n}");
        assert!(n.contains("\n>>>> **[EVE") && !n.contains(">>>>> "), "depth is capped at 4 levels: {n}");
        assert!(n.find("## Replies").unwrap() > n.find("text of p1").unwrap());
        assert!(!n.contains("![["), "no files for replies");
        // the thread ids stay the only ones that count for duplicates
        assert!(!note_has_post(&n, "c1"));
    }

    #[test]
    fn media_names() {
        let p = post("1", "ada", 0, None);
        let m = media(MediaKind::Image, "https://f.test/media/abc.PNG?x=1", "");
        assert_eq!(media_file_name(&p, &m, 3, None), "ada_m.test-3.png");
        let m = Media { file_name_hint: "weird".into(), ..m };
        assert_eq!(media_file_name(&p, &m, 1, Some("image/webp")), "ada_m.test-1.webp");
        assert_eq!(media_file_name(&p, &m, 2, None), "ada_m.test-2.bin");
    }
}
