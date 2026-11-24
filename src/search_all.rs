//! The one search over everything Shale has indexed (NAV-022): the text of notes, the mail index
//! and the post index, as one list of hits grouped by where they come from. Two views show it
//! and differ only in layout, so they differ only in the [`Query`] they ask with: the picker
//! (`Space f g`, a window that closes after a pick) and the sidebar search (Ctrl+Shift+F, narrow,
//! stays open, more lines per note). Both ask [`all_hits`], keep the list in [`Results`] (the
//! debounce of NAV-024) and move through it with [`next_block`] / [`prev_block`] (NAV-008).
//!
//! Wildcards (NAV-023): in notes the whole query is one pattern matched inside a line
//! (`*` any run of characters, `?` one character, case ignored); in mail and posts each word is
//! one term of the FTS5 query and every plain word is a prefix, a "phrase" is exact
//! (`db::fts_query_wild`).
//! Everything is bounded (NAV-024): at most [`Query::limit`] hits per group, and a note search
//! stops scanning when its group is full.
//!
//! The search runs on a worker thread of the window, not on the thread that draws (QUERY-014):
//! the views ask [`SearchWorker`] with a [`Request`] (a [`Snapshot`] of the notes and the places
//! of the mail and post indexes) and [`Results`] takes the list when it arrives.

use crate::app::Action;
use crate::markdown;
use crate::picker::Hit;
use crate::search_sources::{Counts, Kind, Prefixes, Selection};
use crate::social::post_index::PostIndex;
use crate::vault::Vault;
use rusqlite::Connection;
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub mod deps;
pub mod matcher;
pub mod query;
mod snapshot;
pub mod worker;

use matcher::{Budget, Run, Stop};
use query::{Checked, Expr, TermKind};
pub use snapshot::Snapshot;
pub use worker::{Request, SearchWorker, Slot};

/// How many hits one group of the picker lists at most.
pub const GROUP_LIMIT: usize = 20;
/// How many lines of one note the picker lists at most.
pub const PICKER_NOTE_LINES: usize = 3;
/// How many hits one group of the sidebar lists at most.
pub const SIDEBAR_GROUP_LIMIT: usize = 200;
/// How many lines of one note the sidebar lists at most.
pub const SIDEBAR_NOTE_LINES: usize = 20;
/// How many characters a search compares before it ends early with a partial result
/// (QUERY-010). A wildcard comparison counts every step it takes, so `*a*a*a*b` over a long
/// line is not cheap.
pub const CHAR_BUDGET: u64 = 400_000_000;
/// How long a search runs before it ends early (QUERY-010).
pub const TIME_BUDGET: Duration = Duration::from_secs(5);
/// A list is made at most this often while typing (NAV-024).
pub const DEBOUNCE: Duration = Duration::from_millis(150);

/// What to look for and how much of it to list: the text, the size of the answer (which is all
/// that tells the views apart) and the sources to list. Every source is searched and counted
/// (NAV-052); `shown` and a prefix in the text (NAV-050) only choose which are listed.
#[derive(Clone, Copy, Debug)]
pub struct Query<'a> {
    /// what was typed, with its prefix
    pub text: &'a str,
    /// at most this many lines of one note
    pub note_lines: usize,
    /// at most this many hits in one group
    pub limit: usize,
    /// the letters of the prefixes
    pub prefixes: Prefixes,
    /// the sources listed when the text has no prefix: the default selection, or a solo
    pub shown: Selection,
    /// the text as checked (QUERY-007), kept by the view for as long as the text stays the same
    /// (QUERY-013); none: the search checks it itself
    pub checked: Option<&'a Checked>,
}

impl<'a> Query<'a> {
    /// What the picker asks: three lines per note, 20 hits per group.
    pub fn picker(text: &'a str) -> Query<'a> {
        Query { text, note_lines: PICKER_NOTE_LINES, limit: GROUP_LIMIT, prefixes: Prefixes::default(), shown: Selection::ALL, checked: None }
    }

    /// What the sidebar asks: 20 lines per note, up to 200 hits per group.
    pub fn sidebar(text: &'a str) -> Query<'a> {
        Query { text, note_lines: SIDEBAR_NOTE_LINES, limit: SIDEBAR_GROUP_LIMIT, prefixes: Prefixes::default(), shown: Selection::ALL, checked: None }
    }

    /// The same with the letters of the prefixes and the sources to list.
    pub fn with_sources(self, prefixes: Prefixes, shown: Selection) -> Query<'a> {
        Query { prefixes, shown, ..self }
    }

    /// The same with the check of its text, made once for the text (QUERY-013).
    pub fn with_checked(self, checked: &'a Checked) -> Query<'a> {
        Query { checked: Some(checked), ..self }
    }

    /// The check of the text: the one the view made, else made now.
    fn check(&self) -> std::borrow::Cow<'a, Checked> {
        self.checked.map_or_else(|| std::borrow::Cow::Owned(Checked::search(self.text, self.prefixes)), std::borrow::Cow::Borrowed)
    }

    /// Is there nothing to search for (no text apart from the prefix)? The list is empty then.
    pub fn is_blank(&self) -> bool {
        self.prefixes.parse(self.text).text.trim().is_empty()
    }

    /// The sources this query lists: those of its prefix, else `shown` (NAV-050).
    pub fn listed(&self) -> Selection {
        self.prefixes.parse(self.text).prefix.unwrap_or(self.shown)
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

/// Where the match of the pattern at the start of `text` ends (a count of characters), if it
/// matches. The end of the text is not anchored: the match is over when the pattern is. Every
/// step is paid from `fuel`; with none left the pattern does not match (QUERY-010).
fn matches_at(pat: &[char], text: &[char], fuel: &mut u64) -> Option<usize> {
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    loop {
        if *fuel == 0 {
            return None;
        }
        *fuel -= 1;
        if p == pat.len() {
            return Some(t);
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
                _ => return None,
            }
        }
    }
}

/// The bytes of `line` of the first match of the wildcard pattern, ignoring case (a match is
/// the text from where the pattern starts to where it is over, NAV-057). `pat` is lower case
/// already. A pattern without wildcards is the plain search of the sidebar (same offsets). The
/// characters compared are paid from `fuel`, which is empty (0) when the budget is used up.
fn find_wild(line: &str, pat: &str, fuel: &mut u64) -> Option<Range<usize>> {
    if !pat.contains(['*', '?']) {
        *fuel = fuel.saturating_sub(line.len().min(MAX_LINE_CHARS * 4) as u64);
        let start = markdown::find_ci(line, pat)?;
        return Some(start..start + crate::search_render::matched_len(&line[start..], pat));
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
        *fuel = fuel.saturating_sub(text.len() as u64);
        (0..text.len()).find_map(|i| matches_at(&pat, &text[i..], fuel).map(|n| offsets[i]..offsets.get(i + n).copied().unwrap_or(line.len())))
    })
}

/// [`scan_notes`] of the notes of a vault as they are now, without a way to stop it and without
/// a budget.
#[cfg(any(test, feature = "bench"))]
pub fn note_hits(vault: &Vault, query: &Query) -> Vec<Hit> {
    let notes = Snapshot::of(vault);
    let checked = query.check();
    let Some(expr) = checked.expr() else { return vec![] };
    scan_notes(&notes, query, expr, &mut Run::new(&notes, &AtomicBool::new(false), Budget::new(u64::MAX, Duration::from_secs(3600))))
}

/// The notes that match the query, in the order of the vault's files, and for each the lines of
/// its text terms (at most `query.note_lines` per note): a line is one hit; a note whose name
/// matches but no line, or that matches by its fields only, is one hit for the note. A note
/// matches as a whole (QUERY-004); the lines are then those with a text term that is not under a
/// NOT. The scan ends at the next line once `run` says stop, the flag of a newer query
/// (QUERY-017) or the budget (QUERY-010); what it found so far is returned and `run.stopped` says why.
pub fn scan_notes(notes: &Snapshot, query: &Query, expr: &Expr, run: &mut Run) -> Vec<Hit> {
    let texts = expr.positive_texts();
    let mut out = Vec::new();
    for (idx, s) in notes.notes.iter().enumerate() {
        let scanned = run.note_matches(expr, idx).and_then(|matched| if matched { note_lines(s, &texts, query, run, &mut out) } else { Ok(()) });
        if let Err(stop) = scanned {
            run.stopped = Some(stop);
            break;
        }
        if out.len() >= query.limit {
            break;
        }
    }
    out
}

/// The hits of one note that matches: the lines of its text terms, else its name when a text
/// term matches it, else the note itself.
fn note_lines(s: &snapshot::SnapNote, texts: &[&str], query: &Query, run: &mut Run, out: &mut Vec<Hit>) -> Result<(), Stop> {
    let f = &s.file;
    let name = f.display_name();
    let before = out.len();
    if let (Some(n), false) = (&s.note, texts.is_empty()) {
        for (i, line) in n.text.lines().enumerate() {
            let mut found = None;
            for pat in texts {
                if found.is_none() {
                    found = run.find(line, pat)?;
                }
            }
            if let Some(found) = found {
                let shown: String = line.trim().chars().take(crate::search_render::MAX_SHOWN_CHARS).collect();
                // the found text in the shown one: it starts after the white space that is cut off
                let lead = line.len() - line.trim_start().len();
                let marks = shown_marks(&shown, found.start.saturating_sub(lead)..found.end.saturating_sub(lead));
                let (rel, col) = (f.rel.clone(), found.start);
                out.push(Hit::new(NOTES, shown, format!("{name}  ·  line {}", i + 1), Action::GotoMatch { rel, line: i, col }).marked(marks));
                if out.len() >= query.limit || out.len() - before >= query.note_lines {
                    break;
                }
            }
        }
    }
    if out.len() == before {
        let mut found = None;
        for pat in texts {
            if found.is_none() {
                found = run.find(&name, pat)?;
            }
        }
        let open = Action::Open { rel: f.rel.clone(), new_tab: false };
        out.push(match found {
            Some(found) => Hit::new(NOTES, name, "name".into(), open).marked(vec![found]),
            None => Hit::new(NOTES, name, "note".into(), open),
        });
    }
    Ok(())
}

/// The found text `range` as the marks of the shown text: cut at its end, none when it is not in it.
fn shown_marks(shown: &str, range: Range<usize>) -> Vec<Range<usize>> {
    let end = range.end.min(shown.len());
    if range.start < end && shown.is_char_boundary(range.start) && shown.is_char_boundary(end) {
        std::iter::once(range.start..end).collect()
    } else {
        vec![]
    }
}

pub fn mail_hits(db: &Connection, query: &Query) -> Vec<Hit> {
    crate::mail::db::search(db, query.text, None, query.limit, 0)
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            let title = if m.subject.trim().is_empty() { "(no subject)".to_string() } else { m.subject.clone() };
            let marks = crate::search_render::word_marks(&title, query.text);
            Hit::new(MAIL, title, format!("{}  ·  {}", m.from, m.folder), Action::OpenMail(Box::new(m))).marked(marks)
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
            let marks = crate::search_render::word_marks(&h.snippet, query.text);
            Hit::new(POSTS, h.snippet.clone(), format!("{}  ·  @{}  ·  {age}", h.author_name, h.author_handle), Action::OpenPost(Box::new(h))).marked(marks)
        })
        .collect()
}

/// A source that does not take the query, and what in it keeps it out (QUERY-006).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    pub kind: Kind,
    /// the field or operator the source lacks, as it is typed (`status:`, `#`, `OR`)
    pub what: String,
}

impl Skipped {
    /// "Mail: `status:` not supported"
    pub fn message(&self) -> String {
        format!("{}: `{}` not supported", self.kind.group(), self.what)
    }
}

/// What a search found: the hits of every source, in the order of the sources, and how many each
/// source found. No counts: nothing was searched for.
#[derive(Default)]
pub struct Found {
    pub hits: Vec<Hit>,
    pub counts: Option<Counts>,
    /// the work budget was used up: the hits are partial (QUERY-010)
    pub stopped_early: bool,
    /// The notes a board lane matched, in the order of the vault (QUERY-019); a lane asks for these
    /// and gets no hits.
    pub lane: Vec<std::path::PathBuf>,
    /// the sources the query was not asked of, because it has a field they lack (QUERY-006)
    pub skipped: Vec<Skipped>,
}

/// What a source with only text and a few fields takes of a query: its terms side by side, as
/// the text it searches for; or what it lacks. `fields` are the field words of the source.
fn foreign_text(checked: &Checked, expr: &Expr, fields: &[(&str, &str)]) -> Result<String, String> {
    let terms = expr.flat_terms().map_err(str::to_string)?;
    let mut parts = Vec::new();
    for term in terms {
        match &term.kind {
            TermKind::Text(_) => {}
            TermKind::Prop { key, .. } if fields.iter().any(|(f, _)| f.eq_ignore_ascii_case(key)) => {}
            TermKind::Prop { key, .. } => return Err(format!("{key}:")),
            TermKind::Tag(_) => return Err("#".into()),
            TermKind::Path(_) => return Err("path:".into()),
            TermKind::Due(_) => return Err("due:".into()),
            TermKind::Links(_) => return Err("links:".into()),
            TermKind::Blocked => return Err("blocked".into()),
            TermKind::Blocking => return Err("blocking".into()),
        }
        parts.push(checked.source(term));
    }
    Ok(parts.join(" "))
}

/// Search every source for the query, without its prefix: notes, then mail (while the mail
/// store is open), then posts. Which of them are listed is [`Query::listed`]'s to say. None when
/// `cancel` was set: the search was given up and its hits are not to be used (QUERY-017).
pub fn search_in(notes: &Snapshot, mail: Option<&Connection>, posts: Option<&PostIndex>, query: &Query, cancel: &AtomicBool) -> Option<Found> {
    search_within(notes, mail, posts, query, cancel, Budget::standard())
}

/// [`search_in`] with the work budget of the notes given (QUERY-010).
pub fn search_within(notes: &Snapshot, mail: Option<&Connection>, posts: Option<&PostIndex>, query: &Query, cancel: &AtomicBool, budget: Budget) -> Option<Found> {
    let checked = &*query.check();
    // (a query that did not pass the check is not run: the view shows the error, QUERY-008)
    let Some(expr) = checked.expr() else { return Some(Found::default()) };
    let _flow = tracing::info_span!("flow.vault_search", peer = "search_view").entered();
    let mut run = Run::new(notes, cancel, budget);
    let mut hits = tracing::info_span!("flow.vault_search.search_notes", peer = "vault").in_scope(|| scan_notes(notes, query, expr, &mut run));
    let mut counts = Counts::default();
    counts.0[0] = hits.len();
    let mut skipped = Vec::new();
    if let Some(db) = mail {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        match foreign_text(checked, expr, &crate::mail::db::FIELDS) {
            Ok(text) => {
                let mail = tracing::info_span!("flow.vault_search.search_mail", peer = "mail_index").in_scope(|| mail_hits(db, &Query { text: &text, ..*query }));
                counts.0[1] = mail.len();
                hits.extend(mail);
            }
            Err(what) => skipped.push(Skipped { kind: Kind::Mail, what }),
        }
    }
    if let Some(index) = posts {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        match foreign_text(checked, expr, &crate::social::post_index::FIELDS) {
            Ok(text) => {
                let posts = tracing::info_span!("flow.vault_search.search_posts", peer = "post_index").in_scope(|| post_hits(index, &Query { text: &text, ..*query }));
                counts.0[2] = posts.len();
                hits.extend(posts);
            }
            Err(what) => skipped.push(Skipped { kind: Kind::Posts, what }),
        }
    }
    (!cancel.load(Ordering::Relaxed)).then_some(Found { hits, counts: Some(counts), stopped_early: run.stopped == Some(Stop::Budget), lane: Vec::new(), skipped })
}

/// [`search_in`] of what a [`Ctx`] reads, here and now (tests).
#[cfg(test)]
pub fn search(ctx: &Ctx, query: &Query) -> Found {
    search_in(&Snapshot::of(ctx.vault), ctx.mail, Some(ctx.posts), query, &AtomicBool::new(false)).expect("nothing cancels it")
}

/// The hits of the sources the query lists: notes, then mail, then posts (tests).
#[cfg(test)]
pub fn all_hits(ctx: &Ctx, query: &Query) -> Vec<Hit> {
    let listed = query.listed();
    search(ctx, query).hits.into_iter().filter(|h| is_listed(h, listed)).collect()
}

/// Does `listed` have the source of the hit? A hit of a list without sources always counts.
fn is_listed(h: &Hit, listed: Selection) -> bool {
    Kind::of_group(h.group).is_none_or(|k| listed.contains(k))
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
    /// a new list was made (it came from the worker, or the text is empty)
    Made,
    /// the sources to list changed (a button, a key): the same hits, other ones listed
    Relisted,
    /// the list is for an older query or state: a new one is asked for, or not due yet, or on its
    /// way from the worker; ask for a repaint (the sources may have changed as well, and the old
    /// hits are listed as they say)
    Waits,
}

/// The list one view shows: made for a query and a state of the data, kept while neither
/// changes, and asked for again at most every [`DEBOUNCE`] while typing (NAV-024). The worker
/// makes it (QUERY-014): the old list stays until the new one arrives (QUERY-018). Both views of
/// the search hold one. It keeps the hits of the sources that are not listed, so that a button or
/// a key lists them without a new search (NAV-052).
#[derive(Default)]
pub struct Results {
    hits: Vec<Hit>,
    unlisted: Vec<Hit>,
    counts: Option<Counts>,
    /// the list is partial: the search ended early (QUERY-010)
    stopped_early: bool,
    skipped: Vec<Skipped>,
    listed: Option<Selection>,
    made_for: Option<(String, (u64, u64))>,
    made_at: Option<Instant>,
    /// the query on the worker whose list has not arrived
    pending: Option<Pending>,
}

/// A query that is on the worker.
struct Pending {
    id: u64,
    text: String,
    stamp: (u64, u64),
}

/// What a view asks of its [`Results`] in one frame.
pub struct Ask<'a> {
    /// the queue of the worker the query goes to: one per view
    pub slot: Slot,
    pub text: &'a str,
    /// the state of the data, [`Ctx::stamp`]
    pub stamp: (u64, u64),
    /// wait for the typing to settle before the query is asked
    pub debounced: bool,
    /// ask now, whatever the last query was (Enter)
    pub now: bool,
    /// only the hits of these sources are listed
    pub listed: Selection,
    /// there is nothing to search for: the list is empty at once, without the worker
    pub blank: bool,
    /// the text did not pass the check (QUERY-008): nothing is asked, the last valid list stays
    pub invalid: bool,
}

/// How a list is made when it is due.
pub enum Start {
    /// the query was given to the worker (its number): the list comes later
    Queued(u64),
    /// the list is made here and now (the names of files, which are found at once)
    Done(Found),
}

impl Results {
    /// Make the list when it is not the one for the text and the state of the data, not on its
    /// way and either the query is not debounced, or the last query was asked long enough ago, or
    /// `now` asks for it: `start` gives the query to the worker ([`Start::Queued`]) or makes the
    /// list ([`Start::Done`]). A list that came back from the worker is taken. Only the hits of
    /// the sources in `ask.listed` are listed.
    pub fn refresh(&mut self, worker: &mut SearchWorker, ask: &Ask, start: impl FnOnce(&mut SearchWorker) -> Start) -> Refreshed {
        let listed = ask.listed;
        let mut delivered = false;
        if let Some(found) = self.pending.as_ref().and_then(|p| worker.take(ask.slot, p.id)) {
            let p = self.pending.take().expect("a pending query was taken");
            self.keep(found, p.text, p.stamp, listed);
            delivered = true;
        }
        if ask.invalid {
            // (what is on its way is for an older text that was valid: it may still arrive)
            return match (self.pending.is_some(), delivered) {
                (true, _) => Refreshed::Waits,
                (false, true) => Refreshed::Made,
                (false, false) => Refreshed::Unchanged,
            };
        }
        let current = self.made_for.as_ref().is_some_and(|(t, s)| t == ask.text && *s == ask.stamp);
        let queued = self.pending.as_ref().is_some_and(|p| p.text == ask.text && p.stamp == ask.stamp);
        if !current && ask.blank {
            // (what is on its way is not wanted any more)
            worker.cancel(ask.slot);
            self.pending = None;
            self.made_at = Some(Instant::now());
            self.keep(Found::default(), ask.text.to_string(), ask.stamp, listed);
            return Refreshed::Made;
        }
        if !current && !queued && (ask.now || !ask.debounced || self.made_at.is_none_or(|t| t.elapsed() >= DEBOUNCE)) {
            self.made_at = Some(Instant::now());
            match start(worker) {
                Start::Queued(id) => self.pending = Some(Pending { id, text: ask.text.to_string(), stamp: ask.stamp }),
                Start::Done(found) => {
                    self.keep(found, ask.text.to_string(), ask.stamp, listed);
                    delivered = true;
                }
            }
        }
        // (while a new list is on its way, the old one is listed as asked)
        let relisted = self.listed.is_some_and(|l| l != listed);
        if relisted {
            let mut all = std::mem::take(&mut self.hits);
            all.append(&mut self.unlisted);
            all.sort_by_key(|h| Kind::of_group(h.group).map_or(0, |k| k as usize));
            (self.hits, self.unlisted) = all.into_iter().partition(|h| is_listed(h, listed));
            self.listed = Some(listed);
        }
        let current = self.made_for.as_ref().is_some_and(|(t, s)| t == ask.text && *s == ask.stamp);
        match (current, relisted) {
            (false, _) => Refreshed::Waits,
            (true, true) => Refreshed::Relisted,
            (true, false) if delivered => Refreshed::Made,
            (true, false) => Refreshed::Unchanged,
        }
    }

    /// Make `found` the list for `text` and `stamp`.
    fn keep(&mut self, found: Found, text: String, stamp: (u64, u64), listed: Selection) {
        self.counts = found.counts;
        self.stopped_early = found.stopped_early;
        self.skipped = found.skipped;
        (self.hits, self.unlisted) = found.hits.into_iter().partition(|h| is_listed(h, listed));
        self.listed = Some(listed);
        self.made_for = Some((text, stamp));
    }

    /// Is a query on the worker whose list has not arrived (the view shows a spinner)?
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    /// How many hits each source found, for the text of the list; none while nothing is typed.
    pub fn counts(&self) -> Option<&Counts> {
        self.counts.as_ref()
    }

    /// Did the search end early, so that the list is partial (QUERY-010)?
    pub fn stopped_early(&self) -> bool {
        self.stopped_early
    }

    /// The sources of `listed` the query was not asked of, with what they lack (QUERY-006).
    pub fn skipped(&self, listed: Selection) -> impl Iterator<Item = &Skipped> {
        self.skipped.iter().filter(move |s| listed.contains(s.kind))
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

    /// [`find_wild`] without a budget.
    fn wildcard_find(line: &str, pat: &str) -> Option<Range<usize>> {
        let mut fuel = u64::MAX;
        find_wild(line, pat, &mut fuel)
    }

    // spec: NAV-023, NAV-057
    #[test]
    fn wildcards_match_inside_a_line_ignoring_case() {
        assert_eq!(wildcard_find("I like Banana bread", "ban*a"), Some(7..11), "a * matches as little as it can");
        assert_eq!(wildcard_find("I like Banana bread", "b?nana"), Some(7..13));
        assert_eq!(wildcard_find("banana", "ban?na"), Some(0..6));
        assert_eq!(wildcard_find("banana", "ban?n?"), Some(0..6));
        assert_eq!(wildcard_find("banana", "ban??a?"), None, "one character too many");
        assert_eq!(wildcard_find("a long line with end", "long*end"), Some(2..20));
        assert_eq!(wildcard_find("end before long", "long*end"), None, "the order of the parts counts");
        assert_eq!(wildcard_find("anything", "any*"), Some(0..3), "a trailing * is allowed and the end is not anchored: the match ends where the pattern does");
        assert_eq!(wildcard_find("Äpfel und Birnen", "äpf*und"), Some(0..10), "case of non-ASCII letters");
        assert_eq!(wildcard_find("x Äpfel", "ä?fel"), Some(2..8), "byte offsets of the line");
        assert_eq!(wildcard_find("plain text", "text"), Some(6..10), "no wildcard: the sidebar's search");
        assert_eq!(wildcard_find("Straße", "straße"), Some(0..7), "the match is as long as the text it matches");
        assert_eq!(wildcard_find("no star", "a*z"), None);
    }

    // spec: NAV-057
    #[test]
    fn a_note_hit_carries_the_found_text_as_marks_of_its_shown_line() {
        let (v, dir) = fixture();
        let hits = note_hits(&v, &Query::picker("ban*d"));
        let h = hits.iter().find(|h| h.title == "I like banana bread").unwrap();
        assert_eq!(h.marks, vec![7..19], "the wildcard match from where it starts to where it ends");
        assert!(matches!(h.action, Action::GotoMatch { col: 7, .. }), "the column is still where it starts");
        // the white space cut off the line moves the marks with it
        std::fs::write(dir.join("Indent.md"), "      banana here\n").unwrap();
        let v = Vault::open(&dir).unwrap();
        let h = note_hits(&v, &Query::picker("banana")).into_iter().find(|h| h.title == "banana here").unwrap();
        assert_eq!(h.marks, vec![0..6]);
        assert!(matches!(h.action, Action::GotoMatch { col: 6, .. }));
        // a name-only hit marks the name
        let h = note_hits(&v, &Query::picker("ana pl")).into_iter().find(|h| h.title == "banana plans").unwrap();
        assert_eq!(h.marks, vec![3..9]);
        // a match beyond the shown part is not marked
        let long = format!("{} banana", "x".repeat(130));
        std::fs::write(dir.join("Long.md"), long).unwrap();
        let v = Vault::open(&dir).unwrap();
        let h = note_hits(&v, &Query::picker("banana")).into_iter().find(|h| h.title.starts_with("xxx")).unwrap();
        assert!(h.marks.is_empty() && h.title.chars().count() == 120);
        let _ = std::fs::remove_dir_all(&dir);
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

    /// The fixture with two notes that have properties, for the queries of QUERY-002.
    fn properties_fixture() -> (Vault, std::path::PathBuf) {
        let (_, dir) = fixture();
        std::fs::write(dir.join("Task.md"), "---\nstatus: doing\n---\nbanana task\nmore banana\n").unwrap();
        std::fs::write(dir.join("Idle.md"), "---\nstatus: todo\n---\nbanana idle\n").unwrap();
        (Vault::open(&dir).unwrap(), dir)
    }

    // spec: QUERY-002, QUERY-004
    #[test]
    fn query_004_fields_select_notes_and_text_terms_find_the_lines_inside_them() {
        let (v, dir) = properties_fixture();
        let rows = |q: &str| note_hits(&v, &Query::picker(q)).into_iter().map(|h| (h.title, h.detail)).collect::<Vec<_>>();
        let row = |t: &str, d: &str| (t.to_string(), d.to_string());
        assert_eq!(rows("status:doing banana"), [row("banana task", "Task  ·  line 4"), row("more banana", "Task  ·  line 5")], "the lines of the selected note only");
        assert_eq!(rows("status:doing"), [row("Task", "note")], "no text term: the note itself");
        assert_eq!(rows("banana status:todo"), [row("banana idle", "Idle  ·  line 4")], "the order of the terms does not matter");
        let by_field = note_hits(&v, &Query::picker("status:doing"));
        assert!(matches!(&by_field[0].action, Action::Open { rel, new_tab: false } if rel == std::path::Path::new("Task.md")));
        assert!(by_field[0].marks.is_empty());
        assert_eq!(rows("cherry OR status:doing").len(), 2, "a note that matches by a field has no lines to show");
        assert_eq!(rows("cherry -banana"), [row("cherry", "Other  ·  line 1")], "a text term under NOT shows no lines and rules notes out");
        let without = rows("banana AND NOT status:doing AND NOT status:todo");
        assert!(!without.is_empty() && without.iter().all(|r| !r.1.starts_with("Task") && !r.1.starts_with("Idle")), "{without:?}");
        assert!(rows("(banana").is_empty() && rows("banana AND").is_empty(), "a query that is refused finds nothing");
        // what the sidebar and the picker had before: one text, lines, names
        assert_eq!(rows("banana plans"), [row("banana plans", "name")], "a run of words is one phrase");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: QUERY-005
    #[test]
    fn query_005_a_prefix_still_chooses_the_sources_and_the_rest_is_the_language() {
        let (v, dir) = properties_fixture();
        let db = mail_db_with(&[("Banana order".into(), 10)]);
        let posts = PostIndex::in_memory();
        let ctx = Ctx { vault: &v, mail: Some(&db), posts: &posts };
        let q = Query::picker("f:status:doing banana");
        assert_eq!(q.listed(), Selection::only(Kind::Notes));
        let hits = all_hits(&ctx, &q);
        assert_eq!(hits.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(), ["banana task", "more banana"]);
        assert_eq!(all_hits(&ctx, &Query::picker("m: subject:banana")).iter().map(|h| h.title.as_str()).collect::<Vec<_>>(), ["Banana order"], "a prefix, then a field of mail");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: QUERY-006
    #[test]
    fn query_006_a_source_without_the_field_is_skipped_and_says_so() {
        let (v, dir) = properties_fixture();
        let db = mail_db_with(&[("Banana order".into(), 10)]);
        let mut posts = PostIndex::in_memory();
        let mut p = post("p1", "ada", crate::calendar::time::now() - 3600, None);
        p.text_markdown = "banana republic".into();
        posts.add("acct", "home", &[p]);
        let ctx = Ctx { vault: &v, mail: Some(&db), posts: &posts };
        let skipped = |q: &str| search(&ctx, &Query::picker(q)).skipped.iter().map(Skipped::message).collect::<Vec<_>>();
        assert_eq!(skipped("status:doing banana"), ["Mail: `status:` not supported", "Posts: `status:` not supported"]);
        let found = search(&ctx, &Query::picker("status:doing banana"));
        assert_eq!(found.counts.map(|c| (c.get(Kind::Notes), c.get(Kind::Mail), c.get(Kind::Posts))), Some((2, 0, 0)), "skipped, not an empty result: nothing of them is counted or listed");
        assert!(found.hits.iter().all(|h| h.group == NOTES));
        assert_eq!(skipped("#todo"), ["Mail: `#` not supported", "Posts: `#` not supported"]);
        assert_eq!(skipped("path:Fruit/ x"), ["Mail: `path:` not supported", "Posts: `path:` not supported"]);
        assert_eq!(skipped("due:overdue"), ["Mail: `due:` not supported", "Posts: `due:` not supported"]);
        assert_eq!(skipped("blocked"), ["Mail: `blocked` not supported", "Posts: `blocked` not supported"]);
        assert_eq!(skipped("banana OR cherry"), ["Mail: `OR` not supported", "Posts: `OR` not supported"], "the operators too");
        assert_eq!(skipped("banana -cherry"), ["Mail: `NOT` not supported", "Posts: `NOT` not supported"]);
        assert!(skipped("banana").is_empty() && skipped("banana AND republic").is_empty(), "text, side by side or ANDed, they all take");
        assert!(skipped("from:ada banana").is_empty(), "a field they have is passed on");
        assert_eq!(skipped("to:me banana"), ["Posts: `to:` not supported"], "mail has to:, posts have not");
        let mails = |q: &str| search(&ctx, &Query::picker(q)).hits.iter().filter(|h| h.group == MAIL).count();
        assert_eq!((mails("from:ann banana"), mails("from:zed banana")), (1, 0), "from: limits the mail to its sender");
        // no mail store: it is not skipped, it is not there
        let ctx = Ctx { vault: &v, mail: None, posts: &posts };
        assert_eq!(search(&ctx, &Query::picker("status:doing")).skipped.iter().map(Skipped::message).collect::<Vec<_>>(), ["Posts: `status:` not supported"]);
        // the views name only the sources they list
        let mut r = Results::default();
        let mut w = SearchWorker::default();
        let found = Found { skipped: vec![Skipped { kind: Kind::Mail, what: "status:".into() }, Skipped { kind: Kind::Posts, what: "status:".into() }], ..Default::default() };
        r.refresh(&mut w, &ask("x", (1, 1), false, false, Selection::ALL), here(|| found));
        assert_eq!(r.skipped(Selection::only(Kind::Mail)).map(Skipped::message).collect::<Vec<_>>(), ["Mail: `status:` not supported"]);
        assert_eq!(r.skipped(Selection::only(Kind::Notes)).count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: QUERY-010
    #[test]
    fn query_010_a_search_that_uses_up_its_budget_is_partial_and_says_so() {
        let (_, dir) = fixture();
        std::fs::write(dir.join("Long.md"), format!("{}\n", "a".repeat(1900))).unwrap();
        let snap = Snapshot::of(&Vault::open(&dir).unwrap());
        let q = Query::picker("banana");
        let go = AtomicBool::new(false);
        let normal = search_within(&snap, None, None, &q, &go, Budget::standard()).unwrap();
        assert!(!normal.stopped_early && !normal.hits.is_empty());
        let tight = |text: &str| search_within(&snap, None, None, &Query::picker(text), &go, Budget::new(5_000, Duration::from_secs(60))).unwrap();
        let found = tight("*a*a*a*a*a*a*a*b");
        assert!(found.stopped_early, "the wildcards over a long line cost more than the budget");
        assert!(found.hits.is_empty());
        let partial = tight("banana");
        assert!(!partial.stopped_early, "a plain word over the same notes is cheap");
        // the partial list is what the view lists, with the mark
        let mut r = Results::default();
        let mut w = SearchWorker::default();
        r.refresh(&mut w, &ask("x", (1, 1), false, false, Selection::ALL), here(|| found));
        assert!(r.stopped_early());
        r.refresh(&mut w, &ask("y", (1, 1), false, false, Selection::ALL), here(|| partial));
        assert!(!r.stopped_early(), "a new list is complete again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: QUERY-008
    #[test]
    fn query_008_a_text_with_a_syntax_error_asks_nothing_and_the_last_valid_list_stays() {
        let mut r = Results::default();
        let mut w = SearchWorker::default();
        let hits = || Found { hits: vec![hit(NOTES, "a")], ..Default::default() };
        assert_eq!(r.refresh(&mut w, &ask("a", (1, 1), false, false, Selection::ALL), here(hits)), Refreshed::Made);
        let invalid = Ask { invalid: true, ..ask("a AND", (1, 1), false, true, Selection::ALL) };
        let never = |_: &mut SearchWorker| -> Start { panic!("nothing is asked for a query that is refused") };
        assert_eq!(r.refresh(&mut w, &invalid, never), Refreshed::Unchanged, "even when Enter asks for it");
        assert_eq!(r.hits().len(), 1, "the last valid list stays");
        assert!(!r.is_pending());
        // a query on its way, for a text that was valid, is not given up
        let (v, dir) = fixture();
        let posts = PostIndex::in_memory();
        let ctx = Ctx { vault: &v, mail: None, posts: &posts };
        let queue = |w: &mut SearchWorker| Start::Queued(w.submit(Slot::PICKER, Request::new(&ctx, &Query::picker("b"))));
        assert_eq!(r.refresh(&mut w, &ask("b", (1, 1), false, true, Selection::ALL), queue), Refreshed::Waits);
        assert!(r.is_pending());
        assert_eq!(r.refresh(&mut w, &invalid, never), Refreshed::Waits, "it is still on its way");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: QUERY-007
    #[test]
    fn query_007_the_check_of_the_view_goes_with_the_query_to_the_search() {
        let (v, dir) = properties_fixture();
        let checked = Checked::search("status:doing", Prefixes::default());
        let q = Query::picker("something else entirely").with_checked(&checked);
        assert_eq!(note_hits(&v, &q).len(), 1, "the query that was checked is the one that runs");
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
        assert_eq!(mail.len(), 2, "a word is a prefix: banana finds bananas");
        assert_eq!(mail[0].title, "Bananas again", "newest first");
        assert!(matches!(&mail[0].action, Action::OpenMail(m) if m.subject == "Bananas again"));
        let posts_hit = hits.iter().find(|h| h.group == POSTS).unwrap();
        assert_eq!(posts_hit.title, "banana republic");
        assert!(posts_hit.detail.contains("@ada@m.test"), "{}", posts_hit.detail);
        assert!(matches!(&posts_hit.action, Action::OpenPost(h) if h.post.id == "p1"));
        // a prefix finds the plural in mail, newest first
        let subjects = |q: &str| all_hits(&ctx, &Query::picker(q)).into_iter().filter(|h| h.group == MAIL).map(|h| h.title).collect::<Vec<_>>();
        assert_eq!(subjects("banana*"), ["Bananas again", "Banana order"]);
        assert_eq!(subjects("\"banana\""), ["Banana order"], "a phrase stays exact");
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

    // spec: NAV-050, NAV-052
    #[test]
    fn a_prefix_and_the_buttons_choose_the_listed_sources_and_every_source_is_counted() {
        let (v, dir) = fixture();
        let db = mail_db_with(&[("Banana order".into(), 10), ("Banana again".into(), 20)]);
        let mut posts = PostIndex::in_memory();
        let mut p = post("p1", "ada", crate::calendar::time::now() - 3600, None);
        p.text_markdown = "banana republic".into();
        posts.add("acct", "home", &[p]);
        let ctx = Ctx { vault: &v, mail: Some(&db), posts: &posts };
        let sel = |kinds: &[Kind]| kinds.iter().fold(None, |acc: Option<Selection>, k| Some(acc.map_or(Selection::only(*k), |a| a.toggled(*k).unwrap_or(a)))).unwrap();
        let groups = |text: &str, shown: Selection| {
            let q = Query::picker(text).with_sources(Prefixes::default(), shown);
            let mut g: Vec<&str> = all_hits(&ctx, &q).iter().map(|h| h.group).collect();
            g.dedup();
            g
        };
        assert_eq!(groups("banana", Selection::ALL), [NOTES, MAIL, POSTS]);
        assert_eq!(groups("banana", Selection::only(Kind::Mail)), [MAIL], "a solo lists one source");
        assert_eq!(groups("banana", sel(&[Kind::Notes, Kind::Posts])), [NOTES, POSTS], "the default selection lists its sources");
        // a prefix wins over the buttons, and the prefix is not searched for
        assert_eq!(groups("s:banana", Selection::only(Kind::Mail)), [POSTS]);
        assert_eq!(groups("f: banana", Selection::ALL), [NOTES]);
        assert_eq!(groups("fm:banana", Selection::only(Kind::Posts)), [NOTES, MAIL]);
        assert_eq!(groups("m:", Selection::ALL), Vec::<&str>::new(), "a prefix alone: nothing to look for");
        assert_eq!(groups("from:ann banana", Selection::only(Kind::Posts)), [] as [&str; 0], "a field prefix is text: no post of ann here");
        // every source is counted, whatever is listed
        let q = Query::picker("s:banana");
        let found = search(&ctx, &q);
        let counts = found.counts.unwrap();
        assert_eq!((counts.get(Kind::Notes), counts.get(Kind::Mail), counts.get(Kind::Posts)), (3 + 1, 2, 1), "notes: three lines of Fruit and the name; mail 2; posts 1");
        assert_eq!(all_hits(&ctx, &q).len(), 1, "but only posts are listed");
        let none = search(&ctx, &Query::picker("zzzz")).counts.unwrap();
        assert_eq!((none.get(Kind::Notes), none.get(Kind::Mail), none.get(Kind::Posts)), (0, 0, 0), "a source with no hit is counted: 0");
        assert!(search(&ctx, &Query::picker("  ")).counts.is_none(), "nothing typed: no counts");
        // no mail store: no mail count, and a mail prefix lists nothing
        let ctx2 = Ctx { vault: &v, mail: None, posts: &posts };
        assert_eq!(search(&ctx2, &Query::picker("banana")).counts.unwrap().get(Kind::Mail), 0);
        assert!(all_hits(&ctx2, &Query::picker("m:banana")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What a view asks in one frame, for a test.
    fn ask(text: &str, stamp: (u64, u64), debounced: bool, now: bool, listed: Selection) -> Ask<'_> {
        Ask { slot: Slot::PICKER, text, stamp, debounced, now, listed, blank: false, invalid: false }
    }

    /// A list that is made on the spot (the names of files).
    fn here(f: impl FnOnce() -> Found) -> impl FnOnce(&mut SearchWorker) -> Start {
        move |_| Start::Done(f())
    }

    // spec: NAV-052
    #[test]
    fn another_selection_lists_the_kept_hits_without_a_new_search() {
        let mut r = Results::default();
        let mut w = SearchWorker::default();
        let hits = || Found { hits: vec![hit(NOTES, "a"), hit(NOTES, "b"), hit(MAIL, ""), hit(POSTS, "")], counts: Some(Counts([2, 1, 1])), ..Default::default() };
        let groups = |r: &Results| r.hits().iter().map(|h| h.group).collect::<Vec<_>>();
        assert_eq!(r.refresh(&mut w, &ask("x", (1, 1), false, false, Selection::only(Kind::Mail)), here(hits)), Refreshed::Made);
        assert_eq!(groups(&r), [MAIL]);
        assert_eq!(r.counts(), Some(&Counts([2, 1, 1])), "the others are counted");
        assert_eq!(r.refresh(&mut w, &ask("x", (1, 1), false, false, Selection::ALL), |_| panic!("no new search")), Refreshed::Relisted);
        assert_eq!(groups(&r), [NOTES, NOTES, MAIL, POSTS], "back in the order of the sources");
        let two = Selection::only(Kind::Posts).toggled(Kind::Notes).unwrap();
        assert_eq!(r.refresh(&mut w, &ask("x", (1, 1), false, false, two), |_| panic!("no new search")), Refreshed::Relisted);
        assert_eq!(groups(&r), [NOTES, NOTES, POSTS]);
        assert_eq!(r.refresh(&mut w, &ask("x", (1, 1), false, false, two), |_| panic!("no new search")), Refreshed::Unchanged);
        // a list without sources is always listed
        let mut names = Results::default();
        names.refresh(&mut w, &ask("", (1, 1), false, false, Selection::only(Kind::Posts)), here(|| Found { hits: vec![hit("", "")], ..Default::default() }));
        assert_eq!(names.hits().len(), 1);
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
        Hit::new(group, String::new(), String::new(), action)
    }

    fn found(hits: Vec<Hit>) -> Found {
        Found { hits, ..Default::default() }
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
        let mut w = SearchWorker::default();
        let make = || here(|| found(vec![hit(NOTES, "a")]));
        let all = Selection::ALL;
        assert_eq!(r.refresh(&mut w, &ask("", (1, 1), false, false, all), make()), Refreshed::Made);
        assert!(r.is_made());
        assert_eq!(r.refresh(&mut w, &ask("", (1, 1), false, false, all), |_| panic!("the same query and state")), Refreshed::Unchanged);
        assert_eq!(r.refresh(&mut w, &ask("a", (1, 1), true, false, all), make()), Refreshed::Waits, "right after the last list");
        assert!(r.hits().len() == 1);
        assert_eq!(r.refresh(&mut w, &ask("a", (1, 1), true, true, all), here(Found::default)), Refreshed::Made, "Enter asks for the list now");
        assert!(r.hits().is_empty());
        assert_eq!(r.refresh(&mut w, &ask("a", (1, 2), true, false, all), make()), Refreshed::Waits, "another state of the data waits like another query");
        std::thread::sleep(DEBOUNCE + Duration::from_millis(20));
        assert_eq!(r.refresh(&mut w, &ask("a", (1, 2), true, false, all), make()), Refreshed::Made);
        assert_eq!(r.refresh(&mut w, &ask("b", (1, 2), false, false, all), make()), Refreshed::Made, "not debounced: at once");
        assert_eq!(r.take(0).group, NOTES);
    }
}
