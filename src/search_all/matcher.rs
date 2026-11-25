//! Which notes a query matches (QUERY-002…QUERY-004) and how far a search may go (QUERY-010).
//!
//! A [`Run`] is one search over a [`Snapshot`]: it evaluates the tree of a query against a note
//! ([`Run::note_matches`]) and keeps the [`Budget`], the characters compared and the time, and the
//! cancel flag of the worker (QUERY-017), which it checks next to each other: per line of text
//! and per note. When one of them says stop, the search ends with what it has found and the run
//! remembers why ([`Run::stopped`]).
//!
//! A note matches as a whole: the text terms of an AND may be on different lines. The sidebar and
//! the picker go on from there to the lines of the text terms ([`super::scan_notes`]); the lanes
//! of a board list the notes ([`Run::match_notes`]).

use super::deps::{link_key, Deps};
use super::query::{Cmp, Day, Due, Expr, TermKind};
use super::{find_wild, Snapshot, CHAR_BUDGET, TIME_BUDGET};
use crate::calendar::time;
use crate::markdown;
use crate::vault::FileKind;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// What the evaluation of a field costs in characters: the front matter of a note is read.
const FIELD_COST: u64 = 64;

/// What is left of the work a search may do.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub chars: u64,
    deadline: Instant,
}

impl Budget {
    pub fn new(chars: u64, time: Duration) -> Budget {
        Budget { chars, deadline: Instant::now() + time }
    }

    /// [`CHAR_BUDGET`] and [`TIME_BUDGET`], from now on.
    pub fn standard() -> Budget {
        Budget::new(CHAR_BUDGET, TIME_BUDGET)
    }

    fn chars_spent(&self) -> bool {
        self.chars == 0
    }
}

/// Why a search ended before the end of its notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// a newer query gave it up (QUERY-017)
    Cancelled,
    /// the budget is used up (QUERY-010): the result is partial
    Budget,
}

/// One search: the notes it reads, the flag that gives it up, the budget and what it learned of
/// the vault on the way (the dependencies, computed once and only when a query asks for them).
pub struct Run<'a> {
    snapshot: &'a Snapshot,
    cancel: &'a AtomicBool,
    pub budget: Budget,
    /// the day `due:` compares with
    pub today: i64,
    deps: Option<Deps>,
    /// how often the run was checked: the clock is read every 64th time, not per line
    ticks: std::cell::Cell<u32>,
    /// why the search ended early, once it did
    pub stopped: Option<Stop>,
}

impl<'a> Run<'a> {
    pub fn new(snapshot: &'a Snapshot, cancel: &'a AtomicBool, budget: Budget) -> Run<'a> {
        Run { snapshot, cancel, budget, today: time::today(), deps: None, ticks: std::cell::Cell::new(0), stopped: None }
    }

    /// Stop here when a newer query came or the budget is used up.
    pub fn check(&self) -> Result<(), Stop> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(Stop::Cancelled)
        } else if self.budget.chars_spent() || self.late() {
            Err(Stop::Budget)
        } else {
            Ok(())
        }
    }

    /// Is the time limit over? The clock is read at the first check and then every 64th.
    fn late(&self) -> bool {
        let n = self.ticks.get();
        self.ticks.set(n.wrapping_add(1));
        n.is_multiple_of(64) && Instant::now() >= self.budget.deadline
    }

    fn spend(&mut self, n: u64) {
        self.budget.chars = self.budget.chars.saturating_sub(n);
    }

    /// The first place of the wildcard pattern `pat` (lower case) in `line`, ignoring case. The
    /// steps it takes are paid from the budget, also inside one line.
    pub fn find(&mut self, line: &str, pat: &str) -> Result<Option<Range<usize>>, Stop> {
        self.check()?;
        let found = find_wild(line, pat, &mut self.budget.chars);
        if self.budget.chars == 0 {
            return Err(Stop::Budget);
        }
        Ok(found)
    }

    /// Does the note at `idx` of the snapshot match the query? It is looked at as a whole.
    pub fn note_matches(&mut self, expr: &Expr, idx: usize) -> Result<bool, Stop> {
        self.check()?;
        self.spend(1);
        self.matches(expr, idx)
    }

    /// The notes of the snapshot that match, in its order, as a lane of a board lists them
    /// (QUERY-004): the text and the other terms match a note as a whole, not a line. The search
    /// may end early; [`Run::stopped`] says so, and the notes found so far are returned.
    pub fn match_notes(&mut self, expr: &Expr) -> Vec<std::path::PathBuf> {
        let snapshot = self.snapshot;
        let mut out = Vec::new();
        for (idx, note) in snapshot.notes.iter().enumerate() {
            match self.note_matches(expr, idx) {
                Ok(true) => out.push(note.file.rel.clone()),
                Ok(false) => {}
                Err(stop) => {
                    self.stopped = Some(stop);
                    break;
                }
            }
        }
        out
    }

    fn matches(&mut self, expr: &Expr, idx: usize) -> Result<bool, Stop> {
        match expr {
            // (the terms that need no text are cheap: they go first)
            Expr::And(parts) => {
                for part in parts.iter().filter(|p| !p.has_text()).chain(parts.iter().filter(|p| p.has_text())) {
                    if !self.matches(part, idx)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Expr::Or(parts) => {
                for part in parts {
                    if self.matches(part, idx)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Expr::Not(inner) => Ok(!self.matches(inner, idx)?),
            Expr::Term(term) => self.term(&term.kind, idx),
        }
    }

    fn term(&mut self, kind: &TermKind, idx: usize) -> Result<bool, Stop> {
        let snapshot = self.snapshot;
        let note = &snapshot.notes[idx];
        let index = note.note.as_deref();
        Ok(match kind {
            TermKind::Text(pat) => {
                for line in index.iter().flat_map(|n| n.text.lines()) {
                    if self.find(line, pat)?.is_some() {
                        return Ok(true);
                    }
                }
                let name = note.file.display_name();
                self.find(&name, pat)?.is_some()
            }
            TermKind::Tag(tag) => index.is_some_and(|n| {
                n.tags.iter().any(|t| {
                    let t = t.trim_start_matches('#').to_lowercase();
                    t == *tag || t.strip_prefix(tag.as_str()).is_some_and(|rest| rest.starts_with('/'))
                })
            }),
            TermKind::Prop { key, value } => {
                self.spend(FIELD_COST);
                index.is_some_and(|n| markdown::front_matter_values(&n.text).iter().any(|(k, values)| k == key && values.iter().any(|v| v.trim().to_lowercase() == *value)))
            }
            TermKind::Path(prefix) => note.file.rel.to_string_lossy().replace('\\', "/").to_lowercase().starts_with(prefix.as_str()),
            TermKind::Due(due) => {
                self.spend(FIELD_COST);
                index.and_then(|n| due_day(&n.text)).is_some_and(|day| self.due_matches(*due, day))
            }
            TermKind::Links(key) => index.is_some_and(|n| n.links.iter().any(|l| link_key(&l.target) == *key)),
            TermKind::Orphan | TermKind::NoBacklinks => {
                // (one pass over the links when the snapshot has not made its linkage yet)
                self.spend(FIELD_COST + if snapshot.linkage_made() { 0 } else { snapshot.notes.len() as u64 * FIELD_COST });
                let linked = snapshot.linkage();
                note.file.kind != FileKind::Other && if matches!(kind, TermKind::Orphan) { linked.is_orphan(&note.file.rel) } else { linked.has_no_backlinks(&note.file.rel) }
            }
            TermKind::Blocked | TermKind::Blocking => {
                self.spend(snapshot.notes.len() as u64 * FIELD_COST);
                let deps = self.deps.get_or_insert_with(|| Deps::of(snapshot));
                if matches!(kind, TermKind::Blocked) {
                    deps.is_blocked(idx)
                } else {
                    deps.is_blocking(idx)
                }
            }
        })
    }

    fn due_matches(&self, due: Due, day: i64) -> bool {
        match due {
            Due::Overdue => day < self.today,
            Due::Is(cmp, at) => {
                let at = match at {
                    Day::Date(d) => d,
                    Day::FromToday(n) => self.today + n,
                };
                match cmp {
                    Cmp::On => day == at,
                    Cmp::Before => day < at,
                    Cmp::OnOrBefore => day <= at,
                    Cmp::After => day > at,
                    Cmp::OnOrAfter => day >= at,
                }
            }
        }
    }
}

/// The day of the `due:` property of a note: the date at the start of its value (a time after it
/// is left out).
pub(crate) fn due_day(text: &str) -> Option<i64> {
    day_of(text, "due")
}

/// The day of the date property `key` of a note (`due:`, `start:`).
pub(crate) fn day_of(text: &str, key: &str) -> Option<i64> {
    let values = markdown::front_matter_values(text);
    let (_, values) = values.iter().find(|(k, _)| k == key)?;
    let value = values.first()?.trim();
    time::parse_ymd(value.get(..10).unwrap_or(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::property_test_support::{config_with, query_text};
    use crate::search_all::query::Checked;
    use crate::search_sources::Prefixes;
    use crate::vault::Vault;
    use proptest::prelude::*;
    use proptest::test_runner::TestRunner;

    /// A vault of notes (name, text) in a folder of its own.
    fn vault(tag: &str, notes: &[(&str, &str)]) -> (Vault, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("shale-matcher-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for (name, text) in notes {
            let path = d.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        (Vault::open(&d).unwrap(), d)
    }

    /// The names of the notes a query matches as a whole, with `today` as the day number of 2026-11-23.
    fn names(snap: &Snapshot, query: &str) -> Vec<String> {
        let checked = Checked::search(query, Prefixes::default());
        let expr = checked.expr().unwrap_or_else(|| panic!("{query:?}: {:?}", checked.tree));
        let cancel = AtomicBool::new(false);
        let mut run = Run::new(snap, &cancel, Budget::standard());
        run.today = time::parse_ymd("2026-11-23").unwrap();
        let found = run.match_notes(expr);
        assert_eq!(run.stopped, None);
        let mut found: Vec<String> = found.iter().map(|p| crate::vault::rel_to_string(p)).collect();
        found.sort();
        found
    }

    fn fixture(tag: &str) -> (Snapshot, std::path::PathBuf) {
        let (v, d) = vault(
            tag,
            &[
                ("Alpha.md", "---\nstatus: doing\ntype: task\npriority: high\ndue: 2026-11-20\n---\n# Alpha\nmilk and salt\nsecond line pepper #todo\n"),
                ("Beta.md", "---\nstatus: done\ndue: 2026-12-01\ntags: [garden]\n---\nsalt only\nblocked-by: not a field here\n"),
                ("Projects/Gamma.md", "---\nstatus: doing\nblocked-by: [[Alpha]], [[Beta]]\ndue: 2026-11-25T10:00\n---\nlinks to [[Beta]] and [[Missing]]\npepper\n"),
                ("Projects/Delta.md", "---\nstatus: todo\nblocked-by: [[Beta]]\n---\nnothing #todo/sub\n"),
                ("Epsilon.md", "---\nblocked-by: [[Nobody]]\n---\nplain\n"),
            ],
        );
        (Snapshot::of(&v), d)
    }

    // spec: QUERY-002, QUERY-004
    #[test]
    fn query_002_fields_select_notes() {
        let (snap, d) = fixture("fields");
        assert_eq!(names(&snap, "#todo"), ["Alpha.md", "Projects/Delta.md"].map(String::from), "a tag matches its sub tags too");
        assert_eq!(names(&snap, "#garden"), ["Beta.md"], "a tag of the front matter");
        assert_eq!(names(&snap, "#TODO/sub"), ["Projects/Delta.md"]);
        assert_eq!(names(&snap, "status:doing"), ["Alpha.md", "Projects/Gamma.md"].map(String::from));
        assert_eq!(names(&snap, "STATUS:Done"), ["Beta.md"], "key and value ignore case");
        assert_eq!(names(&snap, "priority:HIGH"), ["Alpha.md"]);
        assert_eq!(names(&snap, "type:drawing"), Vec::<String>::new());
        assert_eq!(names(&snap, "path:Projects/"), ["Projects/Delta.md", "Projects/Gamma.md"]);
        assert_eq!(names(&snap, "path:projects/g"), ["Projects/Gamma.md"], "a prefix, case ignored");
        assert_eq!(names(&snap, "links:[[Beta]]"), ["Projects/Delta.md", "Projects/Gamma.md"], "a link of the front matter counts");
        assert_eq!(names(&snap, "links:[[missing]]"), ["Projects/Gamma.md"], "a link to a note that does not exist counts");
        assert_eq!(names(&snap, "salt"), ["Alpha.md", "Beta.md"].map(String::from));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: QUERY-001, QUERY-004
    #[test]
    fn query_004_a_note_matches_as_a_whole_not_a_line() {
        let (snap, d) = fixture("whole");
        assert_eq!(names(&snap, "milk AND pepper"), ["Alpha.md"], "the two words are on different lines of the note");
        assert_eq!(names(&snap, "milk pepper"), Vec::<String>::new(), "a run of words is one phrase");
        assert_eq!(names(&snap, "salt OR pepper"), ["Alpha.md", "Beta.md", "Projects/Gamma.md"].map(String::from));
        assert_eq!(names(&snap, "salt AND NOT milk"), ["Beta.md"]);
        assert_eq!(names(&snap, "salt -milk"), ["Beta.md"]);
        assert_eq!(names(&snap, "(salt OR pepper) AND status:doing"), ["Alpha.md", "Projects/Gamma.md"].map(String::from));
        assert_eq!(names(&snap, "gam*"), ["Projects/Gamma.md"], "the name of a note matches text too, with wildcards");
        assert_eq!(names(&snap, "NOT status:doing").len(), 3, "a note without the property is not 'doing'");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: QUERY-003
    #[test]
    fn query_003_due_compares_with_today() {
        let (snap, d) = fixture("due");
        // today is 2026-11-23: Alpha is due 11-20, Gamma 11-25 (with a time), Beta 12-01
        assert_eq!(names(&snap, "due:overdue"), ["Alpha.md"]);
        assert_eq!(names(&snap, "due:2026-12-01"), ["Beta.md"]);
        assert_eq!(names(&snap, "due:2026-11-25"), ["Projects/Gamma.md"], "a time after the date is left out");
        assert_eq!(names(&snap, "due:<+7d"), ["Alpha.md", "Projects/Gamma.md"].map(String::from));
        assert_eq!(names(&snap, "due:<+8d"), ["Alpha.md", "Projects/Gamma.md"], "before the day, not on it");
        assert_eq!(names(&snap, "due:<=+8d"), ["Alpha.md", "Beta.md", "Projects/Gamma.md"].map(String::from));
        assert_eq!(names(&snap, "due:<=+7d").len(), 2);
        assert_eq!(names(&snap, "due:>-3d"), ["Beta.md", "Projects/Gamma.md"], "-3d is 11-20, which is not after itself");
        assert_eq!(names(&snap, "due:>=-3d").len(), 3);
        assert_eq!(names(&snap, "due:today"), Vec::<String>::new());
        assert_eq!(names(&snap, "NOT due:overdue").len(), 4, "a note without a due date is not overdue");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: QUERY-002
    #[test]
    fn query_002_blocked_and_blocking_follow_the_blockers_that_are_not_done() {
        let (snap, d) = fixture("deps");
        // Gamma waits for Alpha (doing); Beta is done, so it blocks nothing; Delta waits only for Beta
        assert_eq!(names(&snap, "blocked"), ["Projects/Gamma.md"]);
        assert_eq!(names(&snap, "blocking"), ["Alpha.md"], "Alpha holds Gamma back; Beta is done");
        assert_eq!(names(&snap, "blocked AND status:doing"), ["Projects/Gamma.md"]);
        assert_eq!(names(&snap, "NOT blocked").len(), 4);
        assert_eq!(names(&snap, "blocked OR blocking"), ["Alpha.md", "Projects/Gamma.md"].map(String::from));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: QUERY-002
    #[test]
    fn query_002_a_blocker_that_is_done_or_missing_blocks_nothing() {
        let (v, d) = vault("deps-done", &[("A.md", "---\nstatus: done\n---\n"), ("B.md", "---\nblocked-by: [[A]], [[Nobody]]\n---\n"), ("C.md", "---\nblocked-by: [[A]]\nstatus: done\n---\n")]);
        let snap = Snapshot::of(&v);
        assert_eq!(names(&snap, "blocked"), Vec::<String>::new());
        assert_eq!(names(&snap, "blocking"), Vec::<String>::new());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: QUERY-031
    #[test]
    fn query_031_orphan_and_no_backlinks_follow_the_resolved_links_in_and_out() {
        let scene = crate::drawing::Scene::parse(&crate::drawing::SAMPLE_SCENE.replace("\"Idea\"", "\"see [[Linked]]\"")).unwrap();
        let (v, d) = vault(
            "linkage",
            &[
                ("Hub.md", "to [[Linked]] and [[Folder/Deep]]\n"),
                ("Linked.md", "plain\n"),
                ("Folder/Deep.md", "no links\n"),
                ("Alone.md", "nothing\n"),
                ("Myself.md", "see [[Myself]]\n"),
                ("Red.md", "to [[Nobody]] and ![[@Lib/pic.png]]\n"),
                ("Pic.md", "![[photo.png]] and [[doc.pdf]]\n"),
                ("photo.png", "x"),
                ("doc.pdf", "x"),
                ("Draw.excalidraw", &scene.to_json()),
                ("Alone.sync-conflict-20261121-142440-7JJWLPP.md", "to [[Alone]] and [[Red]]\n"),
            ],
        );
        let snap = Snapshot::of(&v);
        assert_eq!(names(&snap, "orphan"), ["Alone.md", "Myself.md", "Pic.md", "Red.md"], "a link to itself, a red link, an outside link and a link to an image or a PDF are no link; a conflict copy links nothing");
        assert_eq!(names(&snap, "no-backlinks"), ["Alone.md", "Draw.excalidraw", "Hub.md", "Myself.md", "Pic.md", "Red.md"], "linking out is fine; a drawing is asked like a note; the image is no note");
        assert_eq!(names(&snap, "NOT orphan AND no-backlinks"), ["Draw.excalidraw", "Hub.md"], "a drawing that links to a note is no orphan");
        assert_eq!(names(&snap, "orphan OR #x"), ["Alone.md", "Myself.md", "Pic.md", "Red.md"]);
        assert_eq!(names(&snap, "orphan AND NOT no-backlinks"), Vec::<String>::new());
        assert_eq!(names(&snap, "\"orphan\""), Vec::<String>::new(), "quoted it is the word");
        assert!(snap.linkage_made());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: QUERY-031
    #[test]
    fn query_031_a_note_linked_only_by_a_drawing_has_a_backlink() {
        let scene = crate::drawing::Scene::parse(&crate::drawing::SAMPLE_SCENE.replace("\"Idea\"", "\"see [[Target]]\"")).unwrap();
        let (v, d) = vault("linkage-drawing", &[("Target.md", "x\n"), ("D.excalidraw", &scene.to_json())]);
        let snap = Snapshot::of(&v);
        assert_eq!(names(&snap, "orphan"), Vec::<String>::new());
        assert_eq!(names(&snap, "no-backlinks"), ["D.excalidraw"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: QUERY-010
    #[test]
    fn query_010_a_search_ends_early_when_the_budget_is_used_up() {
        let long = format!("{}\n", "a".repeat(1900));
        let (v, d) = vault("budget", &[("A.md", &long), ("B.md", &long), ("C.md", "needle\n")]);
        let snap = Snapshot::of(&v);
        let checked = Checked::search("*a*a*a*a*a*a*a*b", Prefixes::default());
        let cancel = AtomicBool::new(false);
        let mut run = Run::new(&snap, &cancel, Budget::new(20_000, Duration::from_secs(60)));
        let found = run.match_notes(checked.expr().unwrap());
        assert_eq!((run.stopped, found.len()), (Some(Stop::Budget), 0), "quadratic work is cut off, and the notes not read are not matched");
        assert_eq!(run.budget.chars, 0);
        // a cheap query over the same notes is within the same budget
        let cheap = Checked::search("needle", Prefixes::default());
        let mut run = Run::new(&snap, &cancel, Budget::new(20_000, Duration::from_secs(60)));
        assert_eq!(run.match_notes(cheap.expr().unwrap()).len(), 1);
        assert_eq!(run.stopped, None);
        // the time limit ends it too
        let mut run = Run::new(&snap, &cancel, Budget::new(u64::MAX, Duration::ZERO));
        run.match_notes(cheap.expr().unwrap());
        assert_eq!(run.stopped, Some(Stop::Budget));
        // and the cancel flag, first
        cancel.store(true, Ordering::Relaxed);
        let mut run = Run::new(&snap, &cancel, Budget::standard());
        assert!(run.match_notes(cheap.expr().unwrap()).is_empty());
        assert_eq!(run.stopped, Some(Stop::Cancelled));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: QUERY-011
    #[test]
    fn query_011_any_query_is_refused_or_ends_within_the_budget_and_never_panics() {
        let pathological = format!("{}b\n", "a".repeat(1900));
        let (v, d) = vault("property", &[("A.md", &pathological), ("B.md", "---\nstatus: doing\ndue: 2026-11-25\nblocked-by: [[A]]\n---\n# B\nmilk #todo [[A]] (x) \"AND\"\n"), ("Dir/C.md", "- [ ] task\nsalt AND pepper\n")]);
        let snap = Snapshot::of(&v);
        let cancel = AtomicBool::new(false);
        let mut runner = TestRunner::new(config_with(256));
        let result = runner.run(&query_text(), |text| {
            let started = Instant::now();
            let checked = Checked::search(&text, Prefixes::default());
            if let Some(expr) = checked.expr() {
                let mut run = Run::new(&snap, &cancel, Budget::new(3_000_000, Duration::from_secs(2)));
                let found = run.match_notes(expr);
                prop_assert!(found.len() <= snap.notes.len());
                prop_assert!(run.budget.chars <= 3_000_000);
            }
            prop_assert!(started.elapsed() < Duration::from_secs(5), "{text:?} took {:?}", started.elapsed());
            Ok(())
        });
        if let Err(e) = result {
            panic!("{e}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
