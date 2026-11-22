//! The one search over everything Shale has indexed (NAV-022): the text of notes, the mail index
//! and the post index, as one list of hits grouped by where they come from. Two views show it
//! and differ only in layout, so they differ only in the [`Query`] they ask with: the picker
//! (`Space f g`, a window that closes after a pick) and the sidebar search (Ctrl+Shift+F, narrow,
//! stays open, more lines per note). Both ask [`all_hits`], keep the list in [`Results`] (the
//! debounce of NAV-024) and move through it with [`next_block`] / [`prev_block`] (NAV-008).
//!
//! Wildcards (NAV-023): in notes the whole query is one pattern matched inside a line
//! (`*` any run of characters, `?` one character, case ignored); in mail and posts each word is
//! one term of the FTS5 query and a wildcard makes the word a prefix (`db::fts_query_wild`).
//! Everything is bounded (NAV-024): at most [`Query::limit`] hits per group, and a note search
//! stops scanning when its group is full.

use crate::app::Action;
use crate::markdown;
use crate::picker::Hit;
use crate::social::post_index::PostIndex;
use crate::vault::Vault;
use rusqlite::Connection;
use std::path::Path;
use std::time::{Duration, Instant};

/// How many hits one group of the picker lists at most.
pub const GROUP_LIMIT: usize = 20;
/// How many lines of one note the picker lists at most.
pub const PICKER_NOTE_LINES: usize = 3;
/// How many hits one group of the sidebar lists at most.
pub const SIDEBAR_GROUP_LIMIT: usize = 200;
/// How many lines of one note the sidebar lists at most.
pub const SIDEBAR_NOTE_LINES: usize = 20;
/// A list is made at most this often while typing (NAV-024).
pub const DEBOUNCE: Duration = Duration::from_millis(150);

/// What to look for and how much of it to list: the text and the size of the answer, which is
/// all that tells the views apart. (A filter by source, #478, is another field here.)
#[derive(Clone, Copy, Debug)]
pub struct Query<'a> {
    pub text: &'a str,
    /// at most this many lines of one note
    pub note_lines: usize,
    /// at most this many hits in one group
    pub limit: usize,
}

impl<'a> Query<'a> {
    /// What the picker asks: three lines per note, 20 hits per group.
    pub fn picker(text: &'a str) -> Query<'a> {
        Query { text, note_lines: PICKER_NOTE_LINES, limit: GROUP_LIMIT }
    }

    /// What the sidebar asks: 20 lines per note, up to 200 hits per group.
    pub fn sidebar(text: &'a str) -> Query<'a> {
        Query { text, note_lines: SIDEBAR_NOTE_LINES, limit: SIDEBAR_GROUP_LIMIT }
    }
}

pub const NOTES: &str = "Notes";
pub const MAIL: &str = "Mail";
pub const POSTS: &str = "Posts";

/// A line is only looked at up to this many characters (a minified file is one huge line).
const MAX_LINE_CHARS: usize = 2000;

/// What the sources of the picker read from.
pub struct Ctx<'a> {
    pub vault: &'a Vault,
    /// the mail index, when the mail store is open
    pub mail: Option<&'a Connection>,
    pub posts: &'a PostIndex,
}

impl Ctx<'_> {
    /// The state of the data the sources read: it changes when a note is edited, added or
    /// removed (or the vault is another). The mail and post indexes do not count their changes,
    /// so a new mail or post shows when the text is typed again.
    pub fn stamp(&self) -> (u64, u64) {
        (self.vault.uid, self.vault.generation)
    }
}

/// Does the pattern match at the start of `text`? The end of the text is not anchored: the match
/// is over when the pattern is.
fn matches_at(pat: &[char], text: &[char]) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    loop {
        if p == pat.len() {
            return true;
        }
        if pat[p] == '*' {
            star = Some((p, t));
            p += 1;
        } else if t < text.len() && (pat[p] == '?' || pat[p] == text[t]) {
            p += 1;
            t += 1;
        } else {
            match star {
                Some((sp, st)) if st < text.len() => {
                    star = Some((sp, st + 1));
                    p = sp + 1;
                    t = st + 1;
                }
                _ => return false,
            }
        }
    }
}

/// The byte offset of the first place in `line` where the wildcard pattern matches, ignoring
/// case. `pat` is lower case already. A pattern without wildcards is the plain search of the
/// sidebar (same offsets).
pub fn wildcard_find(line: &str, pat: &str) -> Option<usize> {
    if !pat.contains(['*', '?']) {
        return markdown::find_ci(line, pat);
    }
    let pat: Vec<char> = pat.chars().collect();
    // the buffers of one line are kept for the next (a search looks at every line of the vault)
    thread_local! {
        static BUFFERS: std::cell::RefCell<(Vec<usize>, Vec<char>)> = const { std::cell::RefCell::new((Vec::new(), Vec::new())) };
    }
    BUFFERS.with_borrow_mut(|(offsets, text)| {
        offsets.clear();
        text.clear();
        for (i, c) in line.char_indices().take(MAX_LINE_CHARS) {
            offsets.push(i);
            text.push(c.to_lowercase().next().unwrap_or(c));
        }
        (0..text.len()).find(|&i| matches_at(&pat, &text[i..])).map(|i| offsets[i])
    })
}

/// The lines of notes that match, in the order of the vault's files (at most `q.note_lines` per
/// note): a line is one hit; a note whose name matches but no line is one hit for the note.
pub fn note_hits(vault: &Vault, query: &Query) -> Vec<Hit> {
    let q = query.text.trim().to_lowercase();
    // a pattern of wildcards alone would match every line
    if !q.chars().any(|c| c != '*' && c != '?') {
        return vec![];
    }
    let mut out = Vec::new();
    for f in &vault.files {
        let name = f.display_name();
        let before = out.len();
        if let Some(n) = vault.notes.get(&f.rel) {
            for (i, line) in n.text.lines().enumerate() {
                if let Some(col) = wildcard_find(line, &q) {
                    let shown: String = line.trim().chars().take(120).collect();
                    out.push(Hit { group: NOTES, title: shown, detail: format!("{name}  ·  line {}", i + 1), action: Action::GotoMatch { rel: f.rel.clone(), line: i, col } });
                    if out.len() >= query.limit || out.len() - before >= query.note_lines {
                        break;
                    }
                }
            }
        }
        if out.len() == before && wildcard_find(&name, &q).is_some() {
            out.push(Hit { group: NOTES, title: name, detail: "name".into(), action: Action::Open { rel: f.rel.clone(), new_tab: false } });
        }
        if out.len() >= query.limit {
            break;
        }
    }
    out
}

pub fn mail_hits(db: &Connection, query: &Query) -> Vec<Hit> {
    crate::mail::db::search_wild(db, query.text, query.limit)
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            let title = if m.subject.trim().is_empty() { "(no subject)".to_string() } else { m.subject.clone() };
            Hit { group: MAIL, title, detail: format!("{}  ·  {}", m.from, m.folder), action: Action::OpenMail(Box::new(m)) }
        })
        .collect()
}

pub fn post_hits(index: &PostIndex, query: &Query) -> Vec<Hit> {
    let now = crate::calendar::time::now();
    index
        .search_wild(query.text, query.limit)
        .into_iter()
        .map(|h| {
            let age = crate::social::feed_ui::age_text(h.created_at, now);
            Hit { group: POSTS, title: h.snippet.clone(), detail: format!("{}  ·  @{}  ·  {age}", h.author_name, h.author_handle), action: Action::OpenPost(Box::new(h)) }
        })
        .collect()
}

/// Notes, then mail, then posts.
pub fn all_hits(ctx: &Ctx, query: &Query) -> Vec<Hit> {
    if query.text.trim().is_empty() {
        return vec![];
    }
    let _flow = tracing::info_span!("flow.vault_search", peer = "search_view").entered();
    let mut out = tracing::info_span!("flow.vault_search.search_notes", peer = "vault").in_scope(|| note_hits(ctx.vault, query));
    if let Some(db) = ctx.mail {
        out.extend(tracing::info_span!("flow.vault_search.search_mail", peer = "mail_index").in_scope(|| mail_hits(db, query)));
    }
    out.extend(tracing::info_span!("flow.vault_search.search_posts", peer = "post_index").in_scope(|| post_hits(ctx.posts, query)));
    out
}

/// The heading of each group of a list with its count: "Notes  3", "Mail  20+" when the group is
/// cut at `limit` (NAV-022). A list without groups has none.
pub fn group_headers(hits: &[Hit], limit: usize) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, usize)> = Vec::new();
    for h in hits.iter().filter(|h| !h.group.is_empty()) {
        match out.iter_mut().find(|(g, _)| *g == h.group) {
            Some((_, n)) => *n += 1,
            None => out.push((h.group, 1)),
        }
    }
    out.into_iter().map(|(g, n)| (g, format!("{g}  {n}{}", if n >= limit { "+" } else { "" }))).collect()
}

/// The block a hit belongs to: the lines of one note are a block (the note), and so is every
/// other group as a whole. Ctrl+J / Ctrl+K move from block to block (NAV-008).
fn block(h: &Hit) -> (&'static str, Option<&Path>) {
    match &h.action {
        Action::GotoMatch { rel, .. } | Action::Open { rel, .. } if h.group == NOTES => (h.group, Some(rel.as_path())),
        _ => (h.group, None),
    }
}

/// The first hit of the block after the one of `hits[sel]` (`sel` itself when there is none).
pub fn next_block(hits: &[Hit], sel: usize) -> usize {
    let Some(cur) = hits.get(sel).map(block) else { return sel };
    (sel..hits.len()).find(|&i| block(&hits[i]) != cur).unwrap_or(sel)
}

/// The first hit of the block before the one of `hits[sel]` (`sel` itself when there is none).
pub fn prev_block(hits: &[Hit], sel: usize) -> usize {
    let Some(cur) = hits.get(sel).map(block) else { return sel };
    let start = (0..=sel).rev().take_while(|&i| block(&hits[i]) == cur).last().unwrap_or(sel);
    let Some(last) = start.checked_sub(1) else { return sel };
    let prev = block(&hits[last]);
    (0..=last).rev().take_while(|&i| block(&hits[i]) == prev).last().unwrap_or(last)
}

/// What [`Results::refresh`] did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refreshed {
    /// the list is for the query and the data: nothing to do
    Unchanged,
    /// a new list was made
    Made,
    /// the list is for an older query or state and a new one is not due yet: ask for a repaint
    Waits,
}

/// The list one view shows: made for a query and a state of the data, kept while neither
/// changes, and made again at most every [`DEBOUNCE`] while typing (NAV-024). Both views of the
/// search hold one.
#[derive(Default)]
pub struct Results {
    hits: Vec<Hit>,
    made_for: Option<(String, (u64, u64))>,
    made_at: Option<Instant>,
}

impl Results {
    /// Make the list with `make` when it is not the one for `text` and `stamp` (the state of the
    /// data, [`Ctx::stamp`]) and either `debounced` is off, or the last list was made long enough
    /// ago, or `now` asks for it.
    pub fn refresh(&mut self, text: &str, stamp: (u64, u64), debounced: bool, now: bool, make: impl FnOnce() -> Vec<Hit>) -> Refreshed {
        if self.made_for.as_ref().is_some_and(|(t, s)| t == text && *s == stamp) {
            return Refreshed::Unchanged;
        }
        if now || !debounced || self.made_at.is_none_or(|t| t.elapsed() >= DEBOUNCE) {
            self.hits = make();
            self.made_for = Some((text.to_string(), stamp));
            self.made_at = Some(Instant::now());
            Refreshed::Made
        } else {
            Refreshed::Waits
        }
    }

    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    /// Has a list been made at all (an empty one is "nothing found")?
    pub fn is_made(&self) -> bool {
        self.made_for.is_some()
    }

    /// Take one hit out of the list (the one that was picked).
    pub fn take(&mut self, i: usize) -> Hit {
        self.hits.swap_remove(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail::db::{self, IndexFields, NewMessage, Op, STATE_FULL};
    use crate::social::tests::post;

    // spec: NAV-023
    #[test]
    fn wildcards_match_inside_a_line_ignoring_case() {
        assert_eq!(wildcard_find("I like Banana bread", "ban*a"), Some(7));
        assert_eq!(wildcard_find("I like Banana bread", "b?nana"), Some(7));
        assert_eq!(wildcard_find("banana", "ban?na"), Some(0));
        assert_eq!(wildcard_find("banana", "ban?n?"), Some(0));
        assert_eq!(wildcard_find("banana", "ban??a?"), None, "one character too many");
        assert_eq!(wildcard_find("a long line with end", "long*end"), Some(2));
        assert_eq!(wildcard_find("end before long", "long*end"), None, "the order of the parts counts");
        assert_eq!(wildcard_find("anything", "any*"), Some(0), "a trailing * is allowed and the end is not anchored");
        assert_eq!(wildcard_find("Äpfel und Birnen", "äpf*und"), Some(0), "case of non-ASCII letters");
        assert_eq!(wildcard_find("x Äpfel", "ä?fel"), Some(2), "byte offsets of the line");
        assert_eq!(wildcard_find("plain text", "text"), Some(6), "no wildcard: the sidebar's search");
        assert_eq!(wildcard_find("no star", "a*z"), None);
    }

    // each call gets its own folder: tests run in parallel and each deletes its folder at the end
    fn fixture() -> (Vault, std::path::PathBuf) {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("shale-searchall-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Fruit.md"), "# Fruit\nI like banana bread\nand a Banana split\nbanana 3\nbanana 4\n").unwrap();
        std::fs::write(dir.join("banana plans.md"), "# Plans\nnothing here\n").unwrap();
        std::fs::write(dir.join("Other.md"), "cherry\n").unwrap();
        (Vault::open(&dir).unwrap(), dir)
    }

    fn mail_db_with(mails: &[(String, i64)]) -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        db::migrate(&c).unwrap();
        let f = db::upsert_folder(&c, "a", "A", "INBOX", Some("/"), None, true).unwrap();
        c.execute("UPDATE folders SET uidvalidity = 1 WHERE id = ?1", [f.id]).unwrap();
        for (i, (subject, date)) in mails.iter().enumerate() {
            let fields = IndexFields { date: Some(*date), from: "ann@example.org".into(), to: "me@example.org".into(), subject: subject.clone(), ..Default::default() };
            let m = NewMessage { folder: f.id, uidvalidity: 1, uid: i as u32 + 1, path: format!("a/f/1/{i}.eml"), state: STATE_FULL, flags: String::new(), size: 10, parsed: true, fields };
            db::apply_batch(&mut c, &[Op::message(m)], None).unwrap();
        }
        c
    }

    // spec: NAV-022
    #[test]
    fn notes_give_lines_and_names() {
        let (v, dir) = fixture();
        let hits = note_hits(&v, &Query::picker("banana"));
        let rows: Vec<(String, String)> = hits.iter().map(|h| (h.title.clone(), h.detail.clone())).collect();
        let by_name = ("banana plans".to_string(), "name".to_string());
        assert!(rows.contains(&by_name), "a note whose name matches and no line: one hit by its name: {rows:?}");
        let fruit: Vec<&(String, String)> = rows.iter().filter(|r| r.1.starts_with("Fruit")).collect();
        assert_eq!(fruit.len(), 3, "at most three lines of a note: {rows:?}");
        assert_eq!(fruit[0], &("I like banana bread".to_string(), "Fruit  ·  line 2".to_string()));
        let line = hits.iter().find(|h| h.title == "I like banana bread").unwrap();
        assert!(matches!(&line.action, Action::GotoMatch { rel, line: 1, col: 7 } if rel == std::path::Path::new("Fruit.md")));
        let name = hits.iter().find(|h| h.detail == "name").unwrap();
        assert!(matches!(&name.action, Action::Open { rel, .. } if rel == std::path::Path::new("banana plans.md")));
        assert_eq!(note_hits(&v, &Query::picker("ban*ead")).len(), 1, "a wildcard");
        assert!(note_hits(&v, &Query::picker("*")).is_empty() && note_hits(&v, &Query::picker(" ?? ")).is_empty() && note_hits(&v, &Query::picker("")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: NAV-022, NAV-024
    #[test]
    fn the_groups_come_in_order_with_what_each_index_finds() {
        let (v, dir) = fixture();
        let db = mail_db_with(&[("Banana order".into(), 10), ("Bananas again".into(), 20), ("Unrelated".into(), 30)]);
        let mut posts = PostIndex::in_memory();
        let mut p = post("p1", "ada", crate::calendar::time::now() - 3600, None);
        p.text_markdown = "banana republic".into();
        posts.add("acct", "home", &[p]);
        let ctx = Ctx { vault: &v, mail: Some(&db), posts: &posts };
        let hits = all_hits(&ctx, &Query::picker("banana"));
        let groups: Vec<&str> = hits.iter().map(|h| h.group).collect();
        let mut sorted = groups.clone();
        sorted.dedup();
        assert_eq!(sorted, [NOTES, MAIL, POSTS], "notes, then mail, then posts");
        let mail: Vec<&Hit> = hits.iter().filter(|h| h.group == MAIL).collect();
        assert_eq!(mail.len(), 1, "without a wildcard whole words only");
        assert_eq!(mail[0].title, "Banana order");
        assert!(matches!(&mail[0].action, Action::OpenMail(m) if m.subject == "Banana order"));
        let posts_hit = hits.iter().find(|h| h.group == POSTS).unwrap();
        assert_eq!(posts_hit.title, "banana republic");
        assert!(posts_hit.detail.contains("@ada@m.test"), "{}", posts_hit.detail);
        assert!(matches!(&posts_hit.action, Action::OpenPost(h) if h.post.id == "p1"));
        // a prefix finds the plural in mail, newest first
        let subjects = |q: &str| all_hits(&ctx, &Query::picker(q)).into_iter().filter(|h| h.group == MAIL).map(|h| h.title).collect::<Vec<_>>();
        assert_eq!(subjects("banana*"), ["Bananas again", "Banana order"]);
        // no mail store open: no mail group, no error
        let ctx = Ctx { vault: &v, mail: None, posts: &posts };
        assert!(all_hits(&ctx, &Query::picker("banana")).iter().all(|h| h.group != MAIL));
        assert!(all_hits(&ctx, &Query::picker("  ")).is_empty());
        // bounded: a group never lists more than its limit
        let many: Vec<(String, i64)> = (0..30).map(|i| (format!("banana {i}"), i)).collect();
        let db = mail_db_with(&many);
        let ctx = Ctx { vault: &v, mail: Some(&db), posts: &posts };
        assert_eq!(all_hits(&ctx, &Query::picker("banana")).iter().filter(|h| h.group == MAIL).count(), GROUP_LIMIT);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: DEV-031
    #[test]
    fn dataflow_vault_search() {
        let (v, dir) = fixture();
        let db = mail_db_with(&[("Banana order".into(), 10)]);
        let mut posts = PostIndex::in_memory();
        let mut p = post("p1", "ada", crate::calendar::time::now() - 3600, None);
        p.text_markdown = "banana republic".into();
        posts.add("acct", "home", &[p]);
        let ctx = Ctx { vault: &v, mail: Some(&db), posts: &posts };
        let flow = crate::dataflow::record(|| {
            assert!(!all_hits(&ctx, &Query::picker("banana")).is_empty());
        });
        crate::dataflow::verify(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/dataflow/vault-search.md"), "vault_search", &crate::dataflow::render_mermaid(&flow));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn hit(group: &'static str, rel: &str) -> Hit {
        let action = if group == NOTES { Action::GotoMatch { rel: rel.into(), line: 0, col: 0 } } else { Action::FocusEditor };
        Hit { group, title: String::new(), detail: String::new(), action }
    }

    // spec: NAV-022
    #[test]
    fn groups_have_a_heading_with_their_count() {
        let hits = [hit("Notes", "a"), hit("Notes", "b"), hit("Posts", "")];
        assert_eq!(group_headers(&hits, GROUP_LIMIT), [("Notes", "Notes  2".to_string()), ("Posts", "Posts  1".to_string())]);
        let full: Vec<Hit> = (0..GROUP_LIMIT).map(|_| hit("Mail", "")).collect();
        assert_eq!(group_headers(&full, GROUP_LIMIT)[0].1, "Mail  20+", "a group at its limit may have more");
        assert_eq!(group_headers(&full, SIDEBAR_GROUP_LIMIT)[0].1, "Mail  20", "the limit is the view's");
        assert!(group_headers(&[hit("", "")], GROUP_LIMIT).is_empty(), "no groups, no headings");
    }

    // spec: NAV-008
    #[test]
    fn blocks_are_the_notes_and_the_other_groups_as_a_whole() {
        // A A B | mail mail | post post
        let hits = [hit(NOTES, "A"), hit(NOTES, "A"), hit(NOTES, "B"), hit(MAIL, ""), hit(MAIL, ""), hit(POSTS, ""), hit(POSTS, "")];
        let next: Vec<usize> = (0..hits.len()).map(|i| next_block(&hits, i)).collect();
        assert_eq!(next, [2, 2, 3, 5, 5, 5, 6], "the first hit of the next file or group; none after the last: stay");
        let prev: Vec<usize> = (0..hits.len()).map(|i| prev_block(&hits, i)).collect();
        assert_eq!(prev, [0, 1, 0, 2, 2, 3, 3], "the first hit of the previous file or group; none before the first: stay");
        assert_eq!((next_block(&[], 0), prev_block(&[], 0)), (0, 0));
    }

    // spec: NAV-022, NAV-023
    #[test]
    fn the_views_ask_with_their_own_size_of_the_answer() {
        let (v, dir) = fixture();
        let many = "banana\n".repeat(30);
        std::fs::write(dir.join("Many.md"), &many).unwrap();
        let v2 = Vault::open(&dir).unwrap();
        let lines = |q: Query| note_hits(&v2, &q).into_iter().filter(|h| h.detail.starts_with("Many")).count();
        assert_eq!((lines(Query::picker("banana")), lines(Query::sidebar("banana"))), (PICKER_NOTE_LINES, SIDEBAR_NOTE_LINES));
        assert_eq!(note_hits(&v, &Query::sidebar("banana")).iter().filter(|h| h.detail.starts_with("Fruit")).count(), 4, "all four lines of a short note");
        // the same hits otherwise: one engine
        let db = mail_db_with(&[("Banana order".into(), 10)]);
        let posts = PostIndex::in_memory();
        let ctx = Ctx { vault: &v, mail: Some(&db), posts: &posts };
        let groups = |q: Query| all_hits(&ctx, &q).iter().map(|h| h.group).collect::<std::collections::BTreeSet<_>>();
        assert_eq!(groups(Query::picker("ban*")), groups(Query::sidebar("ban*")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: NAV-024
    #[test]
    fn a_list_is_made_again_for_another_query_or_state_and_not_more_often_than_the_debounce() {
        let mut r = Results::default();
        let make = || vec![hit(NOTES, "a")];
        assert_eq!(r.refresh("", (1, 1), false, false, make), Refreshed::Made);
        assert!(r.is_made());
        assert_eq!(r.refresh("", (1, 1), false, false, || panic!("the same query and state")), Refreshed::Unchanged);
        assert_eq!(r.refresh("a", (1, 1), true, false, make), Refreshed::Waits, "right after the last list");
        assert!(r.hits().len() == 1);
        assert_eq!(r.refresh("a", (1, 1), true, true, Vec::new), Refreshed::Made, "Enter asks for the list now");
        assert!(r.hits().is_empty());
        assert_eq!(r.refresh("a", (1, 2), true, false, make), Refreshed::Waits, "another state of the data waits like another query");
        std::thread::sleep(DEBOUNCE + Duration::from_millis(20));
        assert_eq!(r.refresh("a", (1, 2), true, false, make), Refreshed::Made);
        assert_eq!(r.refresh("b", (1, 2), false, false, make), Refreshed::Made, "not debounced: at once");
        assert_eq!(r.take(0).group, NOTES);
    }
}
