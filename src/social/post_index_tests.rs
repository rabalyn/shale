//! Tests of the post search index (SOC-030…SOC-034). No network: the posts are made up.

use super::post_index::*;
use super::tests::post;
use super::{Author, LinkCard, Media, MediaKind, Post};

/// A moment `days` days ago.
fn ago(days: i64) -> i64 {
    crate::calendar::time::now() - days * 86_400
}

fn ids(h: &[PostHit]) -> Vec<&str> {
    h.iter().map(|h| h.post.id.as_str()).collect()
}

fn with_text(id: &str, who: &str, at: i64, text: &str) -> Post {
    Post { text_markdown: text.into(), ..post(id, who, at, None) }
}

// spec: SOC-030
#[test]
fn a_post_is_found_by_text_author_link_date_and_thread_context() {
    let mut ix = PostIndex::in_memory();
    let mut parent = with_text("p1", "ada", ago(3), "Plain text files outlive apps");
    parent.author.display_name = "Ada Lovelace".into();
    let mut reply = with_text("p2", "bob", ago(2), "Agreed, [see this](https://example.org/essay) too");
    reply.reply_to = Some("p1".into());
    let mut quoting = with_text("p3", "cy", ago(1), "Read this");
    quoting.quote = Some(Box::new(with_text("q1", "dee", ago(30), "Null references were a mistake")));
    quoting.link_card = Some(LinkCard { url: "https://cards.test/reliability".into(), title: "Reliability essay".into(), description: "Keep it simple".into() });
    quoting.media = vec![Media { kind: MediaKind::Image, url: "u".into(), preview_url: String::new(), alt: "a lighthouse at dusk".into(), file_name_hint: String::new(), blob_cid: None }];
    let mut boosted = with_text("p4", "eve", ago(1), "Merge window is open");
    boosted.reposted_by = Some(Author { display_name: "Fay Booster".into(), handle: "fay@m.test".into(), ..Default::default() });
    let mut warned = with_text("p5", "gil", ago(1), "the forecast was right");
    warned.content_warning = Some("weather talk".into());
    // as a feed brings them: newest first, so the reply comes before the post it answers
    ix.add("acct", "home", &[warned, boosted, quoting, reply, parent]);
    assert_eq!(ix.count(None), 5);
    let find = |q: &str| ix.search_wild(q, 10).iter().map(|h| h.post.id.clone()).collect::<Vec<_>>();
    assert_eq!(find("text:\"outlive apps\""), ["p1"], "the text");
    assert_eq!(find("outlive apps"), ["p2", "p1"], "without a prefix the answer to the post is found too (context)");
    assert_eq!(find("from:lovelace"), ["p1"], "the author's name");
    assert_eq!(find("from:bob@m.test"), ["p2"], "the handle");
    assert_eq!(find("essay"), ["p3", "p2"], "a link address of the text and the title of a link card");
    assert_eq!(find("link:example.org"), ["p2"], "a link address");
    assert_eq!(find("link:p1"), ["p2", "p1"], "the id of the post and the id it replies to");
    assert_eq!(find("link:\"m.test @bob\""), ["p2"], "the address of the post");
    let day = date_text(ago(3));
    assert_eq!(find(&format!("date:{day}")), ["p1"], "the date");
    assert!(find(&format!("date:{}", &day[..7])).contains(&"p1".to_string()), "a month");
    assert_eq!(find("in:reliability"), ["p3"], "the link card is context");
    assert_eq!(find("in:mistake"), ["p3"], "the quoted post is context");
    assert_eq!(find("in:lighthouse"), ["p3"], "a picture's description is context");
    assert_eq!(find("in:booster"), ["p4"], "who boosted");
    assert_eq!(find("in:lovelace"), ["p2"], "the post answered: its author");
    assert_eq!(find("in:\"plain text files\""), ["p2"], "the post answered: its text, though the reply came first in the batch");
    assert_eq!(find("weather"), ["p5"], "the content warning");
    assert_eq!(find("text:weather"), ["p5"]);
    assert!(find("from:weather").is_empty(), "a prefix limits the term to its part");
    let hit = &ix.search_wild("from:lovelace", 5)[0];
    assert_eq!((hit.account.as_str(), hit.feed.as_str(), hit.author_handle.as_str(), hit.author_name.as_str()), ("acct", "home", "ada@m.test", "Ada Lovelace"));
    assert_eq!(hit.snippet, "Plain text files outlive apps");
    assert_eq!(hit.url, "https://m.test/@ada/p1");
    assert_eq!(hit.key, crate::social::state::post_key(&hit.post));
    assert_eq!(ix.search_wild("see this", 5)[0].reply_to.as_deref(), Some("p1"));
}

// spec: SOC-031
#[test]
fn search_ignores_case_and_accents_is_newest_first_and_cannot_be_broken_by_what_is_typed() {
    let mut ix = PostIndex::in_memory();
    ix.add("a", "home", &[with_text("3", "x", ago(1), "Grüße von Müller"), with_text("2", "x", ago(2), "MÜLLER and the Büro"), with_text("1", "x", ago(3), "other")]);
    assert_eq!(ids(&ix.search_wild("muller", 10)), ["3", "2"], "newest first");
    assert!(ix.search_wild("BURO grusse", 10).is_empty(), "all words must match");
    assert_eq!(ids(&ix.search_wild("buro muller", 10)), ["2"]);
    assert_eq!(ix.search_wild("muller", 1).len(), 1, "the limit");
    assert_eq!(ids(&ix.search_wild("\"the buro\"", 10)), ["2"], "a phrase");
    for hostile in ["\"", "*", "a AND", "NEAR(a b)", "-x", "text:", ":::", "(", "from:\"", "OR NOT", "'; DROP TABLE posts; --", "", "   "] {
        assert!(ix.search_wild(hostile, 10).len() <= 3, "{hostile:?} is only text");
    }
    assert_eq!(ix.count(None), 3, "nothing was damaged");
}

// spec: SOC-030
#[test]
fn a_post_loaded_again_is_one_entry_with_the_newest_data_and_a_boost_is_its_own() {
    let mut ix = PostIndex::in_memory();
    let mut p = with_text("1", "ada", ago(1), "first words");
    ix.add("a", "home", &[p.clone()]);
    p.text_markdown = "edited words".into();
    ix.add("a", "list:1", &[p.clone()]);
    assert_eq!(ix.count(None), 1);
    assert!(ix.search_wild("first", 5).is_empty(), "the old text is gone");
    let hits = ix.search_wild("edited", 5);
    assert_eq!((hits.len(), hits[0].feed.as_str()), (1, "list:1"), "the feed seen last");
    let boosted = Post { reposted_by: Some(Author { id: "b".into(), handle: "b@m.test".into(), ..Default::default() }), ..p };
    ix.add("a", "home", &[boosted]);
    assert_eq!(ix.count(None), 2, "the same post boosted by someone shows twice in a feed");
    ix.add("other", "home", &[with_text("1", "ada", ago(1), "edited words")]);
    assert_eq!(ix.count(Some("other")), 1, "the same id of another account is another post");
}

// spec: SOC-032
#[test]
fn the_index_keeps_the_newest_posts_of_each_account_and_nothing_older_than_the_days() {
    let mut ix = PostIndex::in_memory();
    ix.keep_posts = Some(3);
    ix.add("a", "home", &(1..=5).map(|i| with_text(&format!("a{i}"), "x", ago(10 - i), "word")).collect::<Vec<_>>());
    ix.add("b", "home", &[with_text("b1", "x", ago(50), "word")]);
    assert_eq!((ix.count(Some("a")), ix.count(Some("b"))), (3, 1), "a cap per account");
    assert_eq!(ids(&ix.search_wild("word", 10)), ["a5", "a4", "a3", "b1"]);
    assert!(ix.search_wild("link:a1", 10).is_empty(), "the oldest went, with their search entries");
    ix.keep_posts = Some(KEEP_POSTS);
    ix.keep_days = Some(30);
    ix.add("a", "home", &[with_text("old", "x", ago(31), "ancient"), with_text("new", "x", ago(29), "recent")]);
    assert!(ix.search_wild("ancient", 5).is_empty(), "a post older than the days is not taken");
    assert_eq!(ids(&ix.search_wild("recent", 5)), ["new"]);
    assert_eq!(ids(&ix.search_wild("word", 10)), ["a5", "a4", "a3"], "the post of 50 days ago went with the next addition, in the other account too");
    // shrinking the days drops what is now too old at the next addition
    ix.keep_days = Some(20);
    ix.add("a", "home", &[with_text("x", "x", ago(1), "fresh")]);
    assert!(ix.search_wild("recent", 5).is_empty());
    assert_eq!(ix.count(None), 4);
    // a post without a date counts as loaded now
    ix.add("a", "home", &[with_text("u", "x", 0, "timeless")]);
    assert_eq!(ids(&ix.search_wild("timeless", 5)), ["u"]);
}

// spec: SOC-033
#[test]
fn removing_an_account_removes_its_posts_from_the_index_and_the_search() {
    let mut ix = PostIndex::in_memory();
    ix.add("a", "home", &[with_text("1", "x", ago(1), "shared words")]);
    ix.add("b", "home", &[with_text("1", "x", ago(1), "shared words")]);
    ix.remove_account("a");
    assert_eq!((ix.count(Some("a")), ix.count(Some("b"))), (0, 1));
    let hits = ix.search_wild("shared", 10);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].account, "b");
    ix.remove_account("b");
    assert!(ix.search_wild("shared", 10).is_empty() && ix.count(None) == 0);
}

// spec: SOC-034
#[test]
fn the_index_file_keeps_posts_between_runs_and_a_broken_file_is_made_again() {
    let dir = std::env::temp_dir().join(format!("shale-postindex-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("nested").join("posts.db");
    let mut ix = PostIndex::open(&path).expect("opens, also in a folder that is not there yet");
    ix.add("a", "home", &[with_text("1", "x", ago(1), "remember me")]);
    drop(ix);
    let ix = PostIndex::open(&path).unwrap();
    assert_eq!(ids(&ix.search_wild("remember", 5)), ["1"]);
    drop(ix);
    std::fs::write(&path, b"this is not a database, not even close, but long enough to be looked at").unwrap();
    let mut ix = PostIndex::open(&path).expect("a file that is no database is replaced");
    assert_eq!(ix.count(None), 0);
    ix.add("a", "home", &[with_text("2", "x", ago(1), "after the crash")]);
    assert_eq!(ix.search_wild("crash", 5).len(), 1);
    drop(ix);
    // a folder where the file should be: nothing can be done, the feeds go on without search
    assert!(PostIndex::open(&dir).is_none());
    let _ = std::fs::remove_dir_all(&dir);
    assert!(default_path().ends_with("shale/social-posts.db"));
}

// spec: USE-011, USE-022
#[test]
fn the_data_usage_dialog_counts_empties_and_prunes_the_index_file_next_to_the_open_one() {
    let dir = std::env::temp_dir().join(format!("shale-postindex-usage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("posts.db");
    assert_eq!(stats_file(&path), Ok(Default::default()), "no file: nothing, and no file is made");
    assert!(!path.exists());
    let mut ix = PostIndex::open(&path).unwrap();
    ix.keep_days = None;
    ix.add("a", "home", &[with_text("1", "x", ago(400), "very old"), with_text("2", "x", ago(10), "old"), with_text("3", "x", ago(1), "new")]);
    ix.add("b", "home", &[with_text("4", "x", ago(2), "other")]);
    let s = stats_file(&path).unwrap();
    assert_eq!((s.entries, s.oldest.map(|o| o <= ago(399))), (4, Some(true)));
    assert!(s.bytes > 0);
    let pruned = prune_file(&path, Some(365), Some(1), crate::calendar::time::now()).unwrap();
    assert_eq!(pruned.entries, 2, "the post of 400 days, then the oldest over one per account");
    assert_eq!(ids(&ix.search_wild("new", 5)), ["3"], "the window's own connection sees the change");
    assert_eq!(ix.count(None), 2);
    assert_eq!(prune_file(&path, None, None, crate::calendar::time::now()).unwrap().entries, 0, "no limits remove nothing");
    assert_eq!(clear_file(&path).unwrap().entries, 2);
    assert!(ix.search_wild("other", 5).is_empty() && ix.count(None) == 0, "empty for every account");
    ix.add("a", "home", &[with_text("5", "x", ago(1), "after the clear")]);
    assert_eq!(ix.search_wild("clear", 5).len(), 1, "the feeds go on filling it");
    drop(ix);
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: NAV-023
#[test]
fn wildcards_find_posts_by_prefix() {
    let mut ix = PostIndex::in_memory();
    ix.add("a", "home", &[with_text("1", "ada", ago(2), "Plain text outlives apps"), with_text("2", "bob", ago(1), "Textual things")]);
    let find = |q: &str| ids(&ix.search_wild(q, 10)).into_iter().map(String::from).collect::<Vec<_>>();
    assert_eq!(find("text*"), ["2", "1"], "a prefix, newest first");
    assert_eq!(find("tex?"), ["2", "1"], "? cuts the word like *");
    assert_eq!(find("text"), ["2", "1"], "a word is a prefix without a wildcard");
    assert_eq!(find("\"text\""), ["1"], "a phrase is exact");
    assert_eq!(find("from:bo"), ["2"], "a field word is a prefix");
    assert_eq!(find("from:ada* out*"), ["1"], "field prefixes and several words");
    assert!(find("*text").is_empty() && find("").is_empty());
    assert_eq!(ix.search_wild("text*", 1).len(), 1);
}

// spec: SOC-038
#[test]
fn a_parent_that_arrives_later_gives_its_replies_their_context() {
    let mut ix = PostIndex::in_memory();
    let mut reply = with_text("r1", "bob", ago(2), "me too");
    reply.reply_to = Some("p1".into());
    ix.add("acct", "home", &[reply]);
    assert!(ix.search_wild("in:lighthouse", 10).is_empty(), "the parent is not known yet: no context");
    // a feed brings the parent (and the same id of another account) later
    let mut parent = with_text("p1", "ada", ago(3), "A lighthouse keeper writes");
    parent.author.display_name = "Ada Lovelace".into();
    ix.add("other", "home", &[with_text("p1", "zed", ago(3), "lighthouse of the other account")]);
    assert!(ix.search_wild("in:lighthouse", 10).is_empty(), "another account's post is no parent");
    ix.add("acct", "home", &[parent.clone()]);
    assert_eq!(ids(&ix.search_wild("in:lighthouse", 10)), ["r1"], "the text of the parent");
    assert_eq!(ids(&ix.search_wild("in:lovelace", 10)), ["r1"], "the author of the parent");
    assert_eq!(ids(&ix.search_wild("from:bob", 10)), ["r1"], "the reply is still one entry");
    assert_eq!(ix.count(None), 3);
    // the parent changes (an edit): the context follows
    ix.add("acct", "home", &[Post { text_markdown: "A beacon keeper writes".into(), ..parent }]);
    assert!(ix.search_wild("in:lighthouse", 10).is_empty());
    assert_eq!(ids(&ix.search_wild("in:beacon", 10)), ["r1"]);
}

// spec: SOC-040
#[test]
fn clearing_empties_the_index_and_it_fills_again() {
    let mut ix = PostIndex::in_memory();
    let key = crate::social::state::post_key(&post("1", "x", 0, None));
    ix.add("a", "home", &[with_text("1", "x", ago(1), "alpha"), with_text("2", "x", ago(1), "beta")]);
    assert!(ix.contains("a", &key));
    assert!(!ix.contains("b", &key));
    ix.clear();
    assert_eq!(ix.count(None), 0);
    assert!(ix.search_wild("alpha", 10).is_empty());
    assert!(!ix.contains("a", &key));
    ix.add("a", "home", &[with_text("3", "x", ago(1), "gamma")]);
    assert_eq!(ids(&ix.search_wild("gamma", 10)), ["3"]);
}
