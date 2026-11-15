//! The search index of social posts (SOC-030…SOC-035): a small SQLite database with an FTS5 table,
//! in the style of the mail index (`mail_db`: case and accent blind, search text never reaches
//! the FTS5 query language raw, field prefixes). Every post a feed loads is put in; a search finds
//! it and returns what is needed to show a row and to open the hit in its feed.
//!
//! The index is global, not per vault: accounts are global settings (ADR-0005), so their posts
//! belong to the machine and the user, and every vault can search them. It is bounded (see
//! [`KEEP_POSTS`] and [`KEEP_DAYS`]) and only a cache: a file that cannot be read is replaced.

use super::{feed_ui::segments, Post};
use crate::mail_db::{fts_query_in, fts_query_wild};
use rusqlite::{params, Connection};
use std::path::Path;

/// The most posts kept per account; the oldest go first.
pub const KEEP_POSTS: usize = 10_000;
/// Posts older than this many days are not kept (the date of the post counts).
pub const KEEP_DAYS: i64 = 365;

const VERSION: i64 = 1;
/// The prefixes a person may type in front of a term, and the column each one limits it to.
const FIELDS: [(&str, &str); 7] = [("from", "author"), ("text", "text"), ("body", "text"), ("in", "context"), ("context", "context"), ("link", "link"), ("date", "date")];

/// Where the index lives: `<user data folder>/shale/social-posts.db` (SOC-034).
pub fn default_path() -> std::path::PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("shale").join("social-posts.db")
}

/// One search result: a row to show and what is needed to open it.
#[derive(Clone, Debug, PartialEq)]
pub struct PostHit {
    pub account: String,
    /// the feed the post was seen in last
    pub feed: String,
    /// `state::post_key` of the post (identifies it among the posts of the account)
    pub key: String,
    pub author_name: String,
    pub author_handle: String,
    /// seconds since 1970 (UTC), 0 when unknown
    pub created_at: i64,
    /// the start of the text, on one line
    pub snippet: String,
    /// the web address of the post
    pub url: String,
    /// the id of the post this one replies to
    pub reply_to: Option<String>,
    /// the whole post as it was loaded (to show it without the network)
    pub post: Post,
}

pub struct PostIndex {
    conn: Connection,
    pub keep_posts: usize,
    pub keep_days: i64,
}

fn setup(c: &Connection) -> rusqlite::Result<()> {
    let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if v != VERSION {
        // a cache: another version of the layout starts again
        c.execute_batch("DROP TABLE IF EXISTS posts_fts; DROP TABLE IF EXISTS posts;")?;
    }
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS posts (
            id INTEGER PRIMARY KEY,
            account TEXT NOT NULL,
            key TEXT NOT NULL,
            feed TEXT NOT NULL,
            post_id TEXT NOT NULL,
            author_name TEXT NOT NULL,
            author_handle TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            stamp INTEGER NOT NULL,
            url TEXT NOT NULL,
            reply_to TEXT,
            snippet TEXT NOT NULL,
            json TEXT NOT NULL,
            UNIQUE (account, key)
        );
        CREATE INDEX IF NOT EXISTS posts_by_stamp ON posts (account, stamp DESC);
        CREATE INDEX IF NOT EXISTS posts_by_post_id ON posts (account, post_id);
        CREATE INDEX IF NOT EXISTS posts_by_reply_to ON posts (account, reply_to);
        CREATE VIRTUAL TABLE IF NOT EXISTS posts_fts USING fts5 (text, author, context, link, date, tokenize = 'unicode61 remove_diacritics 2');",
    )?;
    c.pragma_update(None, "user_version", VERSION)
}

/// The text of a post without its link addresses, and the addresses (for the `link` column).
fn plain(md: &str) -> (String, Vec<String>) {
    let mut text = String::new();
    let mut links = vec![];
    for (t, l) in segments(md) {
        text.push_str(&t);
        links.extend(l);
    }
    (text, links)
}

/// The text of a post as searched (content warning first) and the addresses in it.
fn searchable(p: &Post) -> (String, Vec<String>) {
    let (text, links) = plain(&p.text_markdown);
    let text = match &p.content_warning {
        Some(w) => format!("{w}\n{text}"),
        None => text,
    };
    (text, links)
}

/// (Re)write the search row of the post at `id`: its text, author, date, links and the context
/// around it, with the parent's author and text when the parent is in the index now.
fn write_fts(tx: &rusqlite::Transaction, account: &str, id: i64, p: &Post) -> rusqlite::Result<()> {
    let (text, mut links) = searchable(p);
    links.push(p.url.clone());
    links.push(p.id.clone());
    let mut context = String::new();
    let mut add = |s: &str| {
        context.push_str(s);
        context.push('\n');
    };
    if let Some(b) = &p.reposted_by {
        add(&format!("boosted by {} {}", b.name(), b.handle));
    }
    if let Some(parent) = &p.reply_to {
        let found: Option<(i64, String, String)> = tx.query_row("SELECT id, author_name, author_handle FROM posts WHERE account = ?1 AND post_id = ?2 LIMIT 1", params![account, parent], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).ok();
        if let Some((pid, name, handle)) = found {
            let parent_text: String = tx.query_row("SELECT text FROM posts_fts WHERE rowid = ?1", [pid], |r| r.get(0)).unwrap_or_default();
            add(&format!("reply to {name} {handle}: {parent_text}"));
        }
        links.push(parent.clone());
    }
    if let Some(q) = &p.quote {
        add(&format!("quoting {} {}: {}", q.author.name(), q.author.handle, plain(&q.text_markdown).0));
        links.push(q.url.clone());
    }
    if let Some(c) = &p.link_card {
        add(&format!("{} {}", c.title, c.description));
        links.push(c.url.clone());
    }
    for m in &p.media {
        add(&m.alt);
    }
    tx.execute("DELETE FROM posts_fts WHERE rowid = ?1", [id])?;
    tx.execute("INSERT INTO posts_fts (rowid, text, author, context, link, date) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![id, text, format!("{} {}", p.author.name(), p.author.handle), context, links.join("\n"), date_text(p.created_at)])?;
    Ok(())
}

/// "2026-11-15" (UTC), searchable by `date:2026-11`.
pub(super) fn date_text(at: i64) -> String {
    if at <= 0 {
        return String::new();
    }
    let (y, m, d) = crate::cal_time::civil_from_days(at.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

fn one_line(s: &str, n: usize) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > n {
        format!("{}…", one.chars().take(n).collect::<String>().trim_end())
    } else {
        one
    }
}

impl PostIndex {
    /// An index that lives in memory only (tests, screenshots).
    pub fn in_memory() -> PostIndex {
        let conn = Connection::open_in_memory().expect("an in-memory database opens");
        setup(&conn).expect("the layout is valid");
        PostIndex { conn, keep_posts: KEEP_POSTS, keep_days: KEEP_DAYS }
    }

    /// The index in a file. A file that cannot be used is removed and made again; None when even
    /// that fails (the feeds work without search).
    pub fn open(path: &Path) -> Option<PostIndex> {
        let try_open = || -> rusqlite::Result<Connection> {
            let c = Connection::open(path)?;
            c.busy_timeout(std::time::Duration::from_secs(5))?;
            c.pragma_update(None, "journal_mode", "WAL")?;
            c.pragma_update(None, "synchronous", "NORMAL")?;
            setup(&c)?;
            // a damaged file shows here, not at the first search
            let ok: String = c.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
            if ok != "ok" {
                return Err(rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT), Some(ok)));
            }
            Ok(c)
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = try_open().or_else(|_| {
            for ext in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
            }
            try_open()
        });
        match conn {
            Ok(conn) => Some(PostIndex { conn, keep_posts: KEEP_POSTS, keep_days: KEEP_DAYS }),
            Err(e) => {
                eprintln!("could not open the post index {}: {e}", path.display());
                None
            }
        }
    }

    /// Put the posts a feed of an account loaded into the index (again: the newest data wins and
    /// the feed is the one seen last), then drop what is over the limits. Never fails the caller;
    /// a problem is reported on standard error and the feeds go on.
    pub fn add(&mut self, account: &str, feed: &str, posts: &[Post]) {
        if let Err(e) = self.try_add(account, feed, posts) {
            eprintln!("post index: {e}");
        }
    }

    fn try_add(&mut self, account: &str, feed: &str, posts: &[Post]) -> rusqlite::Result<()> {
        let now = crate::cal_time::now();
        let cutoff = now - self.keep_days * 86_400;
        let tx = self.conn.transaction()?;
        // oldest first: the post a reply answers is in the index before the reply, so its text is context
        for p in posts.iter().rev() {
            let stamp = if p.created_at > 0 { p.created_at } else { now };
            if stamp < cutoff {
                continue;
            }
            let key = super::state::post_key(p);
            let json = serde_json::to_string(p).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            let (text, _) = searchable(p);
            let id: i64 = tx.query_row(
                "INSERT INTO posts (account, key, feed, post_id, author_name, author_handle, created_at, stamp, url, reply_to, snippet, json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT (account, key) DO UPDATE SET feed = ?3, author_name = ?5, author_handle = ?6, created_at = ?7, stamp = ?8, url = ?9, reply_to = ?10, snippet = ?11, json = ?12
                 RETURNING id",
                params![account, key, feed, p.id, p.author.name(), p.author.handle, p.created_at, stamp, p.url, p.reply_to, one_line(&text, 160), json],
                |r| r.get(0),
            )?;
            write_fts(&tx, account, id, p)?;
            // the replies already indexed get the (new) text of their parent as context (SOC-038)
            let children: Vec<(i64, String)> = {
                let mut st = tx.prepare("SELECT id, json FROM posts WHERE account = ?1 AND reply_to = ?2 AND id <> ?3")?;
                let rows = st.query_map(params![account, p.id, id], |r| Ok((r.get(0)?, r.get(1)?)))?;
                rows.filter_map(Result::ok).collect()
            };
            for (cid, cjson) in children {
                if let Ok(child) = serde_json::from_str::<Post>(&cjson) {
                    write_fts(&tx, account, cid, &child)?;
                }
            }
        }
        // the limits: the days (for every account), then the newest `keep_posts` of each account
        tx.execute("DELETE FROM posts_fts WHERE rowid IN (SELECT id FROM posts WHERE stamp < ?1)", [cutoff])?;
        tx.execute("DELETE FROM posts WHERE stamp < ?1", [cutoff])?;
        let over = "SELECT id FROM posts WHERE account = ?1 AND id NOT IN (SELECT id FROM posts WHERE account = ?1 ORDER BY stamp DESC, id DESC LIMIT ?2)";
        tx.execute(&format!("DELETE FROM posts_fts WHERE rowid IN ({over})"), params![account, self.keep_posts as i64])?;
        tx.execute(&format!("DELETE FROM posts WHERE id IN ({over})"), params![account, self.keep_posts as i64])?;
        tx.commit()
    }

    /// Forget every post of a removed account (SOC-033).
    pub fn remove_account(&mut self, account: &str) {
        let r = (|| -> rusqlite::Result<()> {
            let tx = self.conn.transaction()?;
            tx.execute("DELETE FROM posts_fts WHERE rowid IN (SELECT id FROM posts WHERE account = ?1)", [account])?;
            tx.execute("DELETE FROM posts WHERE account = ?1", [account])?;
            tx.commit()
        })();
        if let Err(e) = r {
            eprintln!("post index: {e}");
        }
    }

    /// Forget every post (Settings, SOC-040).
    pub fn clear(&mut self) {
        if let Err(e) = self.conn.execute_batch("DELETE FROM posts_fts; DELETE FROM posts;") {
            eprintln!("post index: {e}");
        }
    }

    /// Whether the post (a `state::post_key`) of the account is indexed.
    pub fn contains(&self, account: &str, key: &str) -> bool {
        self.conn.query_row("SELECT 1 FROM posts WHERE account = ?1 AND key = ?2", [account, key], |_| Ok(())).is_ok()
    }

    /// How many posts are indexed (all accounts, or one).
    pub fn count(&self, account: Option<&str>) -> usize {
        self.conn.query_row("SELECT COUNT(*) FROM posts WHERE ?1 IS NULL OR account = ?1", [account], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize
    }

    /// The posts that match, newest first, at most `limit`. All words must match (in any column);
    /// `from:`, `text:`, `in:` (quote, link card, the post answered, who boosted), `link:` and
    /// `date:` limit a term to one part; "quotes" make a phrase. Nothing found, nothing to search
    /// for, or a broken index: an empty list.
    pub fn search(&self, query: &str, limit: usize) -> Vec<PostHit> {
        let Some(q) = fts_query_in(query, &FIELDS) else { return vec![] };
        self.search_fts(&q, limit)
    }

    /// [`PostIndex::search`] with the wildcards of `fts_query_wild` (NAV-023).
    pub fn search_wild(&self, query: &str, limit: usize) -> Vec<PostHit> {
        let Some(q) = fts_query_wild(query, &FIELDS) else { return vec![] };
        self.search_fts(&q, limit)
    }

    fn search_fts(&self, q: &str, limit: usize) -> Vec<PostHit> {
        let run = || -> rusqlite::Result<Vec<PostHit>> {
            let mut st = self.conn.prepare(
                "SELECT p.account, p.feed, p.key, p.author_name, p.author_handle, p.created_at, p.snippet, p.url, p.reply_to, p.json
                 FROM posts_fts JOIN posts p ON p.id = posts_fts.rowid WHERE posts_fts MATCH ?1 ORDER BY p.stamp DESC, p.id DESC LIMIT ?2",
            )?;
            let rows = st.query_map(params![q, limit as i64], |r| {
                let json: String = r.get(9)?;
                let post: Post = serde_json::from_str(&json).map_err(|e| rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(e)))?;
                Ok(PostHit { account: r.get(0)?, feed: r.get(1)?, key: r.get(2)?, author_name: r.get(3)?, author_handle: r.get(4)?, created_at: r.get(5)?, snippet: r.get(6)?, url: r.get(7)?, reply_to: r.get(8)?, post })
            })?;
            // a row that cannot be read is left out, not an error for the others
            Ok(rows.filter_map(Result::ok).collect())
        };
        run().unwrap_or_default()
    }
}
