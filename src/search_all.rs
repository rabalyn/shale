//! The search over everything Shale has indexed (`Space f g`, NAV-022): the text of notes, the
//! mail index and the post index, as the hits of one picker list grouped by where they come from.
//!
//! Wildcards (NAV-023): in notes the whole query is one pattern matched inside a line
//! (`*` any run of characters, `?` one character, case ignored); in mail and posts each word is
//! one term of the FTS5 query and a wildcard makes the word a prefix (`mail_db::fts_query_wild`).
//! Everything is bounded (NAV-024): at most [`GROUP_LIMIT`] hits per group, and a note search
//! stops scanning when its group is full.

use crate::app::Action;
use crate::markdown;
use crate::picker::Hit;
use crate::social::post_index::PostIndex;
use crate::vault::Vault;
use rusqlite::Connection;

/// How many hits one group lists at most.
pub const GROUP_LIMIT: usize = 20;

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
    let chars: Vec<(usize, char)> = line.char_indices().take(MAX_LINE_CHARS).map(|(i, c)| (i, c.to_lowercase().next().unwrap_or(c))).collect();
    let text: Vec<char> = chars.iter().map(|(_, c)| *c).collect();
    (0..text.len()).find(|&i| matches_at(&pat, &text[i..])).map(|i| chars[i].0)
}

/// The lines of notes that match, in the order of the vault's files (at most three per note): a
/// line is one hit; a note whose name matches but no line is one hit for the note.
pub fn note_hits(vault: &Vault, query: &str) -> Vec<Hit> {
    let q = query.trim().to_lowercase();
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
                    if out.len() >= GROUP_LIMIT || out.len() - before >= 3 {
                        break;
                    }
                }
            }
        }
        if out.len() == before && wildcard_find(&name, &q).is_some() {
            out.push(Hit { group: NOTES, title: name, detail: "name".into(), action: Action::Open { rel: f.rel.clone(), new_tab: false } });
        }
        if out.len() >= GROUP_LIMIT {
            break;
        }
    }
    out
}

pub fn mail_hits(db: &Connection, query: &str) -> Vec<Hit> {
    crate::mail_db::search_wild(db, query, GROUP_LIMIT)
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            let title = if m.subject.trim().is_empty() { "(no subject)".to_string() } else { m.subject.clone() };
            Hit { group: MAIL, title, detail: format!("{}  ·  {}", m.from, m.folder), action: Action::OpenMail(Box::new(m)) }
        })
        .collect()
}

pub fn post_hits(index: &PostIndex, query: &str) -> Vec<Hit> {
    let now = crate::cal_time::now();
    index
        .search_wild(query, GROUP_LIMIT)
        .into_iter()
        .map(|h| {
            let age = crate::social::feed_ui::age_text(h.created_at, now);
            Hit { group: POSTS, title: h.snippet.clone(), detail: format!("{}  ·  @{}  ·  {age}", h.author_name, h.author_handle), action: Action::OpenPost(Box::new(h)) }
        })
        .collect()
}

/// Notes, then mail, then posts.
pub fn all_hits(ctx: &Ctx, query: &str) -> Vec<Hit> {
    if query.trim().is_empty() {
        return vec![];
    }
    let mut out = note_hits(ctx.vault, query);
    if let Some(db) = ctx.mail {
        out.extend(mail_hits(db, query));
    }
    out.extend(post_hits(ctx.posts, query));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail_db::{self, IndexFields, NewMessage, Op, STATE_FULL};
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

    fn fixture() -> (Vault, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("shale-searchall-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Fruit.md"), "# Fruit\nI like banana bread\nand a Banana split\nbanana 3\nbanana 4\n").unwrap();
        std::fs::write(dir.join("banana plans.md"), "# Plans\nnothing here\n").unwrap();
        std::fs::write(dir.join("Other.md"), "cherry\n").unwrap();
        (Vault::open(&dir).unwrap(), dir)
    }

    fn mail_db_with(mails: &[(String, i64)]) -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        mail_db::migrate(&c).unwrap();
        let f = mail_db::upsert_folder(&c, "a", "A", "INBOX", Some("/"), None, true).unwrap();
        c.execute("UPDATE folders SET uidvalidity = 1 WHERE id = ?1", [f.id]).unwrap();
        for (i, (subject, date)) in mails.iter().enumerate() {
            let fields = IndexFields { date: Some(*date), from: "ann@example.org".into(), to: "me@example.org".into(), subject: subject.clone(), ..Default::default() };
            let m = NewMessage { folder: f.id, uidvalidity: 1, uid: i as u32 + 1, path: format!("a/f/1/{i}.eml"), state: STATE_FULL, flags: String::new(), size: 10, parsed: true, fields };
            mail_db::apply_batch(&mut c, &[Op::message(m)], None).unwrap();
        }
        c
    }

    // spec: NAV-022
    #[test]
    fn notes_give_lines_and_names() {
        let (v, dir) = fixture();
        let hits = note_hits(&v, "banana");
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
        assert_eq!(note_hits(&v, "ban*ead").len(), 1, "a wildcard");
        assert!(note_hits(&v, "*").is_empty() && note_hits(&v, " ?? ").is_empty() && note_hits(&v, "").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: NAV-022, NAV-024
    #[test]
    fn the_groups_come_in_order_with_what_each_index_finds() {
        let (v, dir) = fixture();
        let db = mail_db_with(&[("Banana order".into(), 10), ("Bananas again".into(), 20), ("Unrelated".into(), 30)]);
        let mut posts = PostIndex::in_memory();
        let mut p = post("p1", "ada", crate::cal_time::now() - 3600, None);
        p.text_markdown = "banana republic".into();
        posts.add("acct", "home", &[p]);
        let ctx = Ctx { vault: &v, mail: Some(&db), posts: &posts };
        let hits = all_hits(&ctx, "banana");
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
        let subjects = |q: &str| all_hits(&ctx, q).into_iter().filter(|h| h.group == MAIL).map(|h| h.title).collect::<Vec<_>>();
        assert_eq!(subjects("banana*"), ["Bananas again", "Banana order"]);
        // no mail store open: no mail group, no error
        let ctx = Ctx { vault: &v, mail: None, posts: &posts };
        assert!(all_hits(&ctx, "banana").iter().all(|h| h.group != MAIL));
        assert!(all_hits(&ctx, "  ").is_empty());
        // bounded: a group never lists more than its limit
        let many: Vec<(String, i64)> = (0..30).map(|i| (format!("banana {i}"), i)).collect();
        let db = mail_db_with(&many);
        let ctx = Ctx { vault: &v, mail: Some(&db), posts: &posts };
        assert_eq!(all_hits(&ctx, "banana").iter().filter(|h| h.group == MAIL).count(), GROUP_LIMIT);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
