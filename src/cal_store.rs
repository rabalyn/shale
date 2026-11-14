//! Calendar sources (local .ics files and CalDAV calendars), their loaded events, background
//! refresh of remote calendars, and sample events for trying the views out.

use crate::cal_dav::{self, Account};
use crate::cal_ics::{self, Event};
use crate::cal_time::{self as t, days_from_civil};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SourceKind {
    /// an .ics file on this computer
    Local { file: PathBuf },
    /// a calendar on a CalDAV server
    /// (the password is kept in the system keyring when there is one; see `secrets`)
    Caldav { url: String, username: String, password: String },
}

impl std::fmt::Debug for SourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceKind::Local { file } => f.debug_struct("Local").field("file", file).finish(),
            SourceKind::Caldav { url, username, password } => f.debug_struct("Caldav").field("url", url).field("username", username).field("password", &cal_dav::redacted(password)).finish(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CalendarSource {
    pub id: String,
    pub name: String,
    /// "#rrggbb"
    pub color: String,
    pub enabled: bool,
    /// CalDAV: load the calendar again every this many minutes (0 = only on start / by hand)
    #[serde(default = "default_sync")]
    pub sync_minutes: u32,
    #[serde(flatten)]
    pub kind: SourceKind,
}

/// Auto-sync interval for new CalDAV calendars, in minutes.
pub const DEFAULT_SYNC_MINUTES: u32 = 15;

fn default_sync() -> u32 {
    DEFAULT_SYNC_MINUTES
}

/// A background save or delete. Holds the result once it is done: Ok(message) or Err(reason).
pub type Job = Arc<Mutex<Option<Result<String, String>>>>;

/// Take the result of a finished job.
pub fn job_result(j: &Job) -> Option<Result<String, String>> {
    j.lock().unwrap().take()
}

fn run_job(f: impl FnOnce() -> Result<String, String> + Send + 'static, ctx: Option<eframe::egui::Context>) -> Job {
    let job: Job = Arc::new(Mutex::new(None));
    let j2 = job.clone();
    std::thread::spawn(move || {
        let r = f();
        *j2.lock().unwrap() = Some(r);
        if let Some(c) = ctx {
            c.request_repaint();
        }
    });
    job
}

/// A new server address / version tag for the event with this uid, learned from a write.
type Fix = (String, String, String, Option<String>);

/// The result of loading one CalDAV calendar.
#[derive(Debug)]
struct Loaded {
    /// None: unchanged since the last load
    events: Option<Vec<Event>>,
    tag: Option<String>,
    read_only: Option<bool>,
}

/// Where a calendar's events are written.
#[derive(Clone)]
enum Target {
    Local { file: PathBuf, name: String },
    Dav { url: String, acct: Account },
}

fn local_read(file: &std::path::Path) -> Result<Vec<Event>, String> {
    match std::fs::read_to_string(file) {
        Ok(t) => Ok(cal_ics::parse_ics(&t)),
        Err(_) if !file.exists() => Ok(vec![]),
        Err(e) => Err(format!("could not read {}: {e}", file.display())),
    }
}

fn local_write(file: &std::path::Path, name: &str, events: &[Event]) -> Result<(), String> {
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(file, cal_ics::to_ics(name, events)).map_err(|e| format!("could not save {}: {e}", file.display()))
}

/// Write an event into a calendar. `parts`: the event first, then the other parts of the same
/// series (changed single dates), which are always kept together. `in_place`: the event is
/// already in this calendar and is replaced; otherwise it is created.
/// For CalDAV: returns the event's address and new version tag.
fn write_to(target: &Target, parts: &[Event], in_place: bool) -> Result<Option<(String, Option<String>)>, String> {
    let Some(e) = parts.first() else { return Ok(None) };
    match target {
        Target::Local { file, name } => {
            let mut evs = local_read(file)?;
            evs.retain(|x| x.uid != e.uid);
            evs.extend(parts.iter().map(|p| Event { href: None, etag: None, ..p.clone() }));
            local_write(file, name, &evs).map(|_| None)
        }
        Target::Dav { url, acct } => {
            let existing = match (in_place, e.href.as_deref()) {
                (true, Some(h)) => Some((h, e.etag.as_deref())),
                (true, None) => return Err("the event's address on the server is unknown; refresh the calendar and try again".into()),
                (false, _) => None,
            };
            cal_dav::put_event(url, acct, parts, existing).map(Some)
        }
    }
}

fn remove_from(target: &Target, e: &Event) -> Result<(), String> {
    match target {
        Target::Local { file, name } => {
            let mut evs = local_read(file)?;
            evs.retain(|x| x.uid != e.uid);
            local_write(file, name, &evs)
        }
        Target::Dav { acct, .. } => match &e.href {
            Some(h) => cal_dav::delete_event(h, acct, e.etag.as_deref()),
            None => Err("the event's address on the server is unknown; refresh the calendar and try again".into()),
        },
    }
}

impl CalendarSource {
    pub fn is_local(&self) -> bool {
        matches!(self.kind, SourceKind::Local { .. })
    }
}

pub const PALETTE: [&str; 8] = ["#5c9cff", "#e8590c", "#2f9e44", "#ae3ec9", "#f59f00", "#e03131", "#1098ad", "#868e96"];

/// One visible occurrence, pointing back at its event.
#[derive(Clone, Debug, PartialEq)]
pub struct Occ {
    pub source: usize,
    pub event: usize,
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
}

impl Occ {
    pub fn start_day(&self) -> i64 {
        if self.all_day {
            self.start.div_euclid(86_400)
        } else {
            t::utc_to_local(self.start).0
        }
    }
    pub fn end_day(&self) -> i64 {
        if self.all_day {
            ((self.end - 1).div_euclid(86_400)).max(self.start_day())
        } else {
            t::utc_to_local((self.end - 1).max(self.start)).0
        }
    }
}

type Pending = Arc<Mutex<Vec<(String, Result<Loaded, String>)>>>;

pub struct CalendarStore {
    pub sources: Vec<CalendarSource>,
    /// events per source id
    pub events: HashMap<String, Vec<Event>>,
    /// last error / info per source id
    pub status: HashMap<String, String>,
    pending: Pending,
    inflight: HashSet<String>,
    /// calendars to load once more when their running load is done (changed meanwhile)
    again: HashSet<String>,
    /// when each CalDAV calendar was last loaded (its own sync timer)
    last_sync: HashMap<String, Instant>,
    /// version tag of each CalDAV calendar at its last load, and the day the loaded range starts
    known: HashMap<String, (String, i64)>,
    /// CalDAV calendars the server says this account can't change
    pub read_only: HashSet<String>,
    /// addresses / version tags learned from writes, applied in `poll`
    fixes: Arc<Mutex<Vec<Fix>>>,
    /// bumped whenever events change
    pub generation: u64,
    /// where new local calendars are created
    pub local_dir: PathBuf,
}

impl CalendarStore {
    pub fn new(sources: Vec<CalendarSource>, local_dir: PathBuf) -> Self {
        let mut s = CalendarStore {
            sources,
            events: HashMap::new(),
            status: HashMap::new(),
            pending: Default::default(),
            inflight: HashSet::new(),
            again: HashSet::new(),
            last_sync: HashMap::new(),
            known: HashMap::new(),
            read_only: HashSet::new(),
            fixes: Default::default(),
            generation: 0,
            local_dir,
        };
        s.reload_local();
        s
    }

    /// Use a changed list of calendars. CalDAV calendars that are new, newly shown or point
    /// somewhere else are loaded; the others keep their events and sync timers.
    pub fn set_sources(&mut self, sources: Vec<CalendarSource>) {
        let fresh: Vec<String> =
            sources.iter().filter(|n| n.enabled && !n.is_local()).filter(|n| !self.sources.iter().any(|o| o.id == n.id && o.enabled && o.kind == n.kind) || !self.events.contains_key(&n.id)).map(|n| n.id.clone()).collect();
        self.sources = sources;
        self.events.retain(|id, _| self.sources.iter().any(|s| s.id == *id));
        self.last_sync.retain(|id, _| self.sources.iter().any(|s| s.id == *id));
        for id in &fresh {
            self.known.remove(id);
        }
        self.known.retain(|id, _| self.sources.iter().any(|s| s.id == *id));
        self.read_only.retain(|id| self.sources.iter().any(|s| s.id == *id));
        self.reload_local();
        for id in fresh {
            self.refresh_source(&id);
        }
    }

    pub fn reload_local(&mut self) {
        for s in &self.sources {
            if let SourceKind::Local { file } = &s.kind {
                match std::fs::read_to_string(file) {
                    Ok(text) => {
                        self.events.insert(s.id.clone(), cal_ics::parse_ics(&text));
                        self.status.remove(&s.id);
                    }
                    Err(_) if !file.exists() => {
                        self.events.insert(s.id.clone(), vec![]);
                    }
                    Err(e) => {
                        self.status.insert(s.id.clone(), format!("could not read {}: {e}", file.display()));
                    }
                }
            }
        }
        self.generation += 1;
    }

    /// Fetch all enabled CalDAV calendars in the background.
    pub fn refresh_remote(&mut self) {
        let ids: Vec<String> = self.sources.iter().filter(|s| s.enabled && !s.is_local()).map(|s| s.id.clone()).collect();
        for id in ids {
            self.refresh_source(&id);
        }
    }

    /// Load one calendar again (CalDAV in the background; local files right away).
    pub fn refresh_source(&mut self, id: &str) {
        let Some(s) = self.sources.iter().find(|s| s.id == id && s.enabled).cloned() else { return };
        let SourceKind::Caldav { url, username, password } = &s.kind else {
            self.reload_local();
            return;
        };
        self.last_sync.insert(s.id.clone(), Instant::now());
        if !self.inflight.insert(s.id.clone()) {
            // a load is running that may have started before a change: load again after it
            self.again.insert(s.id.clone());
            return;
        }
        self.status.insert(s.id.clone(), "syncing…".into());
        let today = t::today();
        let (from, to) = ((today - 120) * 86_400, (today + 400) * 86_400);
        // the loaded range moves with the days: a calendar loaded on another day is loaded fully
        let known = self.known.get(&s.id).filter(|(_, day)| *day == today).map(|(tag, _)| tag.clone());
        let (id, url, acct, pending) = (s.id.clone(), url.clone(), Account { url: url.clone(), username: username.clone(), password: password.clone() }, self.pending.clone());
        std::thread::spawn(move || {
            // a failing state request (old servers) just means: load everything
            let st = cal_dav::calendar_state(&url, &acct).unwrap_or_default();
            let r = if st.tag.is_some() && st.tag == known {
                Ok(Loaded { events: None, tag: st.tag, read_only: st.read_only })
            } else {
                cal_dav::fetch_events(&url, &acct, from, to).map(|ev| Loaded { events: Some(ev), tag: st.tag, read_only: st.read_only })
            };
            pending.lock().unwrap().push((id, r));
        });
    }

    /// Load a calendar again now, completely (also when the server says nothing changed).
    pub fn sync_now(&mut self, id: &str) {
        self.known.remove(id);
        self.refresh_source(id);
    }

    /// Apply the addresses / version tags learned from finished writes.
    fn apply_fixes(&mut self) -> bool {
        let fixes: Vec<Fix> = std::mem::take(&mut *self.fixes.lock().unwrap());
        let changed = !fixes.is_empty();
        for (source, uid, href, etag) in fixes {
            for e in self.events.get_mut(&source).into_iter().flatten().filter(|e| e.uid == uid) {
                e.href = Some(href.clone());
                e.etag = etag.clone();
            }
        }
        changed
    }

    /// CalDAV calendars whose own sync timer has run out (or that were never loaded).
    pub fn due_sources(&self, now: Instant) -> Vec<String> {
        self.sources
            .iter()
            .filter(|s| s.enabled && !s.is_local() && !self.inflight.contains(&s.id))
            .filter(|s| match self.last_sync.get(&s.id) {
                None => true,
                Some(_) if s.sync_minutes == 0 => false,
                Some(last) => now.saturating_duration_since(*last) >= Duration::from_secs(s.sync_minutes as u64 * 60),
            })
            .map(|s| s.id.clone())
            .collect()
    }

    /// Take finished downloads and start the syncs that are due. True if anything changed.
    pub fn poll(&mut self) -> bool {
        for id in self.due_sources(Instant::now()) {
            self.refresh_source(&id);
        }
        let done: Vec<_> = std::mem::take(&mut *self.pending.lock().unwrap());
        let changed = !done.is_empty() | self.apply_fixes();
        for (id, r) in done {
            self.inflight.remove(&id);
            match r {
                Ok(sync) => {
                    let (_, secs) = t::utc_to_local(t::now());
                    match sync.read_only {
                        Some(true) => {
                            self.read_only.insert(id.clone());
                        }
                        Some(false) => {
                            self.read_only.remove(&id);
                        }
                        None => {}
                    }
                    match sync.tag {
                        Some(tag) => {
                            self.known.insert(id.clone(), (tag, t::today()));
                        }
                        None => {
                            self.known.remove(&id);
                        }
                    }
                    if let Some(ev) = sync.events {
                        self.events.insert(id.clone(), ev);
                    }
                    let n = self.events.get(&id).map(|v| v.len()).unwrap_or(0);
                    let ro = if self.read_only.contains(&id) { " · read-only" } else { "" };
                    self.status.insert(id.clone(), format!("{n} events{ro} · synced {}", t::hhmm(secs)));
                }
                Err(e) => {
                    self.status.insert(id, e);
                }
            }
        }
        let again: Vec<String> = self.again.iter().filter(|id| !self.inflight.contains(*id)).cloned().collect();
        for id in again {
            self.again.remove(&id);
            self.refresh_source(&id);
        }
        if changed {
            self.generation += 1;
        }
        changed
    }

    pub fn is_loading(&self) -> bool {
        !self.inflight.is_empty()
    }

    /// Occurrences touching the local days [from_day, to_day], all-day first, then by start.
    pub fn occurrences(&self, from_day: i64, to_day: i64) -> Vec<Occ> {
        let mut out = Vec::new();
        for (si, s) in self.sources.iter().enumerate().filter(|(_, s)| s.enabled) {
            let Some(evs) = self.events.get(&s.id) else { continue };
            // occurrences that replace one occurrence of a repeating event
            let overrides: HashSet<(String, i64)> = evs.iter().filter_map(|e| e.recurrence_id.map(|r| (e.uid.clone(), r))).collect();
            for (ei, e) in evs.iter().enumerate() {
                let (from, to) = if e.all_day { (from_day * 86_400, (to_day + 1) * 86_400) } else { (t::local_to_utc(from_day, 0), t::local_to_utc(to_day + 1, 0)) };
                for o in cal_ics::occurrences(e, from, to) {
                    if e.recurrence_id.is_none() && e.rrule.is_some() && overrides.contains(&(e.uid.clone(), o.start)) {
                        continue;
                    }
                    out.push(Occ { source: si, event: ei, start: o.start, end: o.end, all_day: e.all_day });
                }
            }
        }
        out.sort_by(|a, b| b.all_day.cmp(&a.all_day).then(a.start_day().cmp(&b.start_day())).then(a.start.cmp(&b.start)));
        out
    }

    pub fn event(&self, o: &Occ) -> Option<&Event> {
        self.sources.get(o.source).and_then(|s| self.events.get(&s.id)).and_then(|v| v.get(o.event))
    }

    /// A new, empty local calendar (file created in `local_dir`).
    pub fn new_local_source(&self, name: &str, existing: &[CalendarSource]) -> CalendarSource {
        let id = format!("local-{}", t::now() * 1000 + existing.len() as i64);
        let color = PALETTE[existing.len() % PALETTE.len()].to_string();
        let file = self.local_dir.join(format!("{id}.ics"));
        CalendarSource { id, name: if name.trim().is_empty() { "My calendar".into() } else { name.trim().into() }, color, enabled: true, sync_minutes: DEFAULT_SYNC_MINUTES, kind: SourceKind::Local { file } }
    }

    fn write_local(&mut self, id: &str, events: Vec<Event>) -> Result<(), String> {
        let Some(s) = self.sources.iter().find(|s| s.id == id) else { return Err("no such calendar".into()) };
        let SourceKind::Local { file } = &s.kind else { return Err("only local calendars can be edited here".into()) };
        if let Some(d) = file.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(file, cal_ics::to_ics(&s.name, &events)).map_err(|e| e.to_string())?;
        self.events.insert(id.to_string(), events);
        self.generation += 1;
        Ok(())
    }

    fn target(&self, id: &str) -> Result<(Target, String), String> {
        let s = self.sources.iter().find(|s| s.id == id).ok_or("that calendar is no longer configured")?;
        let t = match &s.kind {
            SourceKind::Local { file } => Target::Local { file: file.clone(), name: s.name.clone() },
            SourceKind::Caldav { url, username, password } => Target::Dav { url: url.clone(), acct: Account { url: url.clone(), username: username.clone(), password: password.clone() } },
        };
        Ok((t, s.name.clone()))
    }

    /// Save an event in the background.
    /// - new event (`original` None): created in `target_id`;
    /// - edited in the same calendar: replaced there;
    /// - moved to another calendar: written to the new calendar FIRST, and only when that
    ///   worked removed from the old one — so a failure never loses the event.
    pub fn save_event(&self, target_id: &str, event: Event, original: Option<(String, Event)>, ctx: Option<eframe::egui::Context>) -> Result<Job, String> {
        let (target, target_name) = self.target(target_id)?;
        if self.read_only.contains(target_id) {
            return Err(format!("“{target_name}” is read-only; choose another calendar"));
        }
        let moved_from = match &original {
            Some((sid, old)) if sid != target_id => {
                if self.read_only.contains(sid) {
                    return Err("the event's calendar is read-only, so it can't be moved out of it".into());
                }
                Some((self.target(sid)?, old.clone()))
            }
            _ => None,
        };
        let in_place = original.is_some() && moved_from.is_none();
        // the rest of a repeating event's series travels with it
        let mut parts = vec![event.clone()];
        if let Some((sid, _)) = &original {
            parts.extend(self.events.get(sid).into_iter().flatten().filter(|x| x.uid == event.uid && x.recurrence_id != event.recurrence_id).cloned());
        }
        if !in_place {
            for p in &mut parts {
                p.href = None;
                p.etag = None;
            }
        }
        let (fixes, fix_source, uid) = (self.fixes.clone(), target_id.to_string(), event.uid.clone());
        Ok(run_job(
            move || {
                let written = write_to(&target, &parts, in_place).map_err(|e| format!("Could not save to “{target_name}”: {e}"))?;
                if let Some((href, etag)) = written {
                    fixes.lock().unwrap().push((fix_source, uid, href, etag));
                }
                if let Some(((old_target, old_name), old)) = moved_from {
                    // the event is safe in the new calendar either way
                    return Ok(match remove_from(&old_target, &old) {
                        Ok(()) => format!("Moved to “{target_name}”"),
                        Err(e) => format!("⚠ Saved to “{target_name}”, but could not remove it from “{old_name}”: {e}. The event is now in both calendars."),
                    });
                }
                Ok(format!("Saved to “{target_name}”"))
            },
            ctx,
        ))
    }

    /// Remove an event in the background (local or CalDAV).
    pub fn delete_event(&self, source_id: &str, event: Event, ctx: Option<eframe::egui::Context>) -> Result<Job, String> {
        let (target, name) = self.target(source_id)?;
        if self.read_only.contains(source_id) {
            return Err(format!("“{name}” is read-only"));
        }
        Ok(run_job(move || remove_from(&target, &event).map(|_| "Event deleted".to_string()).map_err(|e| format!("Could not delete the event from “{name}”: {e}")), ctx))
    }

    /// After a save / delete: load the touched calendars again.
    pub fn after_write(&mut self, ids: &[String]) {
        if self.apply_fixes() {
            self.generation += 1;
        }
        self.reload_local();
        for id in ids {
            if self.sources.iter().any(|s| s.id == *id && !s.is_local()) {
                self.sync_now(id);
            }
        }
    }

    /// The stored event with this uid in a calendar.
    #[cfg(test)]
    pub fn find(&self, source_id: &str, uid: &str) -> Option<Event> {
        self.events.get(source_id).and_then(|v| v.iter().find(|e| e.uid == uid && e.recurrence_id.is_none()).or_else(|| v.iter().find(|e| e.uid == uid))).cloned()
    }

    /// Fill a local calendar with sample events around today.
    pub fn fill_sample(&mut self, id: &str) -> Result<usize, String> {
        let mut evs = self.events.get(id).cloned().unwrap_or_default();
        let sample = sample_events(t::today());
        let n = sample.len();
        evs.extend(sample);
        self.write_local(id, evs)?;
        Ok(n)
    }
}

/// Where local calendar files go (`~/.config/shale/calendars`).
pub fn default_local_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("shale").join("calendars")
}

fn timed(uid: &str, day: i64, h: i64, m: i64, mins: i64, summary: &str) -> Event {
    let start = t::local_to_utc(day, h * 3600 + m * 60);
    Event { uid: uid.into(), summary: summary.into(), start, end: start + mins * 60, ..Default::default() }
}

fn all_day(uid: &str, day: i64, days: i64, summary: &str) -> Event {
    Event { uid: uid.into(), summary: summary.into(), start: day * 86_400, end: (day + days) * 86_400, all_day: true, ..Default::default() }
}

/// Sample events around `today`: a normal day, a very busy day, overlapping meetings, all-day
/// and multi-day events, and repeating events.
pub fn sample_events(today: i64) -> Vec<Event> {
    let stamp = t::now();
    let id = |n: &str| format!("sample-{n}-{stamp}");
    let monday = today - t::weekday(today) as i64;
    let mut v = vec![
        Event { rrule: Some("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR;COUNT=60".into()), description: "Quick round: yesterday, today, blockers.".into(), location: "Video call".into(), ..timed(&id("standup"), monday, 9, 0, 15, "Team standup") },
        Event {
            location: "Room 2.14".into(),
            description: "Walk through the new layout.\nBring the printed mock-ups.".into(),
            organizer: "Ada Lovelace <ada@example.com>".into(),
            attendees: vec!["Grace Hopper <grace@example.com>".into(), "Alan Turing <alan@example.com>".into()],
            ..timed(&id("review"), today, 10, 0, 90, "Design review")
        },
        Event { location: "Café Lindgren".into(), ..timed(&id("lunch"), today, 12, 30, 60, "Lunch with Sam") },
        Event { description: "No meetings — deep work.".into(), ..timed(&id("focus"), today, 14, 0, 120, "Focus time") },
        timed(&id("supplier"), today, 15, 0, 30, "Call with supplier"),
        timed(&id("gym"), today, 18, 0, 60, "Gym"),
        Event {
            url: "https://example.com/webinar".into(),
            organizer: "Events team <events@example.com>".into(),
            description: "Join with the link below.".into(),
            ..timed(&id("webinar"), today + 3, 16, 0, 60, "Webinar: Writing better notes")
        },
        Event { location: "Messe Berlin".into(), ..all_day(&id("conf"), today + 7, 3, "Conference") },
        all_day(&id("holiday"), today + 14, 1, "Public holiday"),
        Event { rrule: Some("FREQ=WEEKLY".into()), location: "Library".into(), ..timed(&id("book"), monday + 3, 19, 0, 90, "Book club") },
        Event {
            rrule: Some("FREQ=MONTHLY".into()),
            ..all_day(
                &id("rent"),
                {
                    let (y, m, _) = t::civil_from_days(today);
                    days_from_civil(y, m, 1)
                },
                1,
                "Pay rent",
            )
        },
        all_day(&id("deadline"), today + 1, 1, "Report deadline"),
    ];
    // a busy day with many short meetings
    let busy = today + 2;
    for (i, name) in ["Inbox zero", "1:1 with Grace", "Budget sync", "Interview", "Vendor demo", "Roadmap", "Customer call", "Retro", "Wrap-up"].iter().enumerate() {
        v.push(timed(&id(&format!("busy{i}")), busy, 8 + i as i64, if i % 2 == 0 { 0 } else { 15 }, 45, name));
    }
    v.push(timed(&id("overlap"), busy, 11, 30, 60, "Overlapping workshop"));
    v
}

#[cfg(test)]
pub fn file_of(s: &CalendarSource) -> Option<&std::path::Path> {
    match &s.kind {
        SourceKind::Local { file } => Some(file),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-cal-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    // spec: CAL-001
    #[test]
    fn local_calendar_sample_add_delete() {
        let dir = tmp("local");
        let mut store = CalendarStore::new(vec![], dir.clone());
        let src = store.new_local_source("Test", &[]);
        store.set_sources(vec![src.clone()]);
        let n = store.fill_sample(&src.id).unwrap();
        assert!(n > 10);
        assert!(file_of(&src).unwrap().exists(), "saved as an .ics file");
        let today = t::today();
        let occ = store.occurrences(today, today);
        // 5 one-off events today, plus the standup on weekdays (Monday = 0 … Friday = 4)
        assert!(occ.len() >= 5, "today has several events, got {}", occ.len());
        let standup = occ.iter().any(|o| store.events[&src.id][o.event].summary == "Team standup");
        assert_eq!(standup, t::weekday(today) < 5, "the standup repeats on weekdays only");
        let busy = store.occurrences(today + 2, today + 2);
        assert!(busy.len() >= 10, "the busy day is busy");
        // a reload from the file gives the same events
        let mut again = CalendarStore::new(vec![src.clone()], dir.clone());
        again.reload_local();
        assert_eq!(again.events[&src.id].len(), store.events[&src.id].len());
        // add and delete
        let e = Event { uid: "mine".into(), summary: "Dentist".into(), start: t::local_to_utc(today + 5, 8 * 3600), end: t::local_to_utc(today + 5, 9 * 3600), ..Default::default() };
        wait(&store.save_event(&src.id, e.clone(), None, None).unwrap()).unwrap();
        store.after_write(&[src.id.clone()]);
        assert!(store.occurrences(today + 5, today + 5).iter().any(|o| store.event(o).unwrap().summary == "Dentist"));
        wait(&store.delete_event(&src.id, e, None).unwrap()).unwrap();
        store.after_write(&[src.id.clone()]);
        assert!(!store.occurrences(today + 5, today + 5).iter().any(|o| store.event(o).unwrap().summary == "Dentist"));
        // disabled calendars are hidden
        let mut off = src.clone();
        off.enabled = false;
        store.set_sources(vec![off]);
        assert!(store.occurrences(today, today).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    pub(crate) fn wait(j: &Job) -> Result<String, String> {
        for _ in 0..500 {
            if let Some(r) = job_result(j) {
                return r;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("job did not finish")
    }

    fn dav_source(id: &str, url: String) -> CalendarSource {
        CalendarSource { id: id.into(), name: id.into(), color: "#1971c2".into(), enabled: true, sync_minutes: DEFAULT_SYNC_MINUTES, kind: SourceKind::Caldav { url, username: "ada".into(), password: "x".into() } }
    }

    // spec: CAL-002
    #[test]
    fn each_calendar_has_its_own_sync_timer() {
        let mut a = dav_source("a", "http://127.0.0.1:1/a/".into());
        a.sync_minutes = 5;
        let mut b = dav_source("b", "http://127.0.0.1:1/b/".into());
        b.sync_minutes = 30;
        let mut off = dav_source("off", "http://127.0.0.1:1/c/".into());
        off.sync_minutes = 0;
        let mut store = CalendarStore::new(vec![a, b, off], tmp("timers"));
        let now = Instant::now();
        assert_eq!(store.due_sources(now).len(), 3, "never loaded: all are due");
        store.last_sync.insert("a".into(), now);
        store.last_sync.insert("b".into(), now);
        store.last_sync.insert("off".into(), now);
        assert!(store.due_sources(now + Duration::from_secs(4 * 60)).is_empty());
        assert_eq!(store.due_sources(now + Duration::from_secs(6 * 60)), vec!["a".to_string()], "only the 5-minute calendar");
        assert_eq!(store.due_sources(now + Duration::from_secs(31 * 60)), vec!["a".to_string(), "b".to_string()], "0 = no automatic sync");
        // old settings files without the field get the default
        let s: CalendarSource = serde_json::from_str(r##"{"id":"x","name":"X","color":"#fff","enabled":true,"type":"caldav","url":"u","username":"","password":""}"##).unwrap();
        assert_eq!(s.sync_minutes, 15);
    }

    // spec: CAL-003
    #[test]
    fn moving_an_event_writes_first_and_removes_after() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        // calendar "rw" accepts writes and removals; "ro" refuses writes
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let l2 = log.clone();
        let (base, _h) = crate::cal_dav::tests::mock_http(40, move |method, path, _, _| {
            l2.lock().unwrap().push(format!("{method} {path}"));
            match (method, path) {
                ("PUT", p) if p.starts_with("/ro/") => (403, String::new()),
                ("PUT", _) => (201, String::new()),
                ("DELETE", _) => (204, String::new()),
                _ => (404, String::new()),
            }
        });
        let local_dir = tmp("move");
        let mut store = CalendarStore::new(vec![], local_dir);
        let local = store.new_local_source("Home", &[]);
        store.set_sources(vec![local.clone(), dav_source("rw", format!("{base}/rw/")), dav_source("ro", format!("{base}/ro/"))]);
        // a local event
        let e = Event { uid: "u1".into(), summary: "Plan".into(), start: 1_800_000_000, end: 1_800_001_800, ..Default::default() };
        wait(&store.save_event(&local.id, e.clone(), None, None).unwrap()).unwrap();
        store.after_write(&[local.id.clone()]);
        // move it to the read-only calendar: refused, and it is still in the local calendar
        let err = wait(&store.save_event("ro", e.clone(), Some((local.id.clone(), e.clone())), None).unwrap()).unwrap_err();
        assert!(err.contains("permission"), "{err}");
        store.reload_local();
        assert!(store.find(&local.id, "u1").is_some(), "nothing lost when the new calendar refuses");
        // move it to the writable calendar: written there, then removed locally
        let ok = wait(&store.save_event("rw", e.clone(), Some((local.id.clone(), e.clone())), None).unwrap()).unwrap();
        assert!(ok.starts_with("Moved"), "{ok}");
        store.reload_local();
        assert!(store.find(&local.id, "u1").is_none());
        // and from the server calendar back to the local one: local write first, then DELETE
        let on_server = Event { href: Some(format!("{base}/rw/u1.ics")), etag: Some("\"e\"".into()), ..e.clone() };
        wait(&store.save_event(&local.id, on_server.clone(), Some(("rw".into(), on_server)), None).unwrap()).unwrap();
        store.reload_local();
        assert!(store.find(&local.id, "u1").is_some());
        // (the PROPFINDs / REPORTs in between are the calendars syncing)
        let calls: Vec<String> = log.lock().unwrap().iter().filter(|c| !c.starts_with("REPORT") && !c.starts_with("PROPFIND")).cloned().collect();
        assert_eq!(calls, vec!["PUT /ro/u1.ics".to_string(), "PUT /rw/u1.ics".into(), "DELETE /rw/u1.ics".into()]);
    }

    // spec: CAL-002
    #[test]
    fn a_change_during_a_sync_loads_again() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let count = Arc::new(Mutex::new(0usize));
        let c2 = count.clone();
        let (base, _h) = crate::cal_dav::tests::mock_http(10, move |m, _, _, _| {
            if m == "REPORT" {
                *c2.lock().unwrap() += 1;
                std::thread::sleep(Duration::from_millis(150));
            }
            (207, r#"<d:multistatus xmlns:d="DAV:"/>"#.to_string())
        });
        let mut store = CalendarStore::new(vec![], tmp("again"));
        store.set_sources(vec![dav_source("a", format!("{base}/a/"))]);
        // saved while the first load is still running
        store.after_write(&["a".to_string()]);
        for _ in 0..300 {
            store.poll();
            if *count.lock().unwrap() >= 2 && !store.is_loading() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(*count.lock().unwrap(), 2, "loaded once more after the running load");
        // changing only the colour doesn't load it again
        let mut s = store.sources.clone();
        s[0].color = "#000000".into();
        store.set_sources(s);
        assert!(!store.is_loading());
    }

    // spec: JOB-001, CAL-002
    #[test]
    fn caldav_source_loads_in_the_background() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let today = t::today();
        let (y, m, d) = t::civil_from_days(today);
        let ics = format!("BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:r1\r\nDTSTART;VALUE=DATE:{y:04}{m:02}{d:02}\r\nSUMMARY:Remote day\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n");
        let (base, _h) = crate::cal_dav::tests::mock_server(ics);
        let src = CalendarSource {
            id: "dav1".into(),
            name: "Work".into(),
            color: "#1971c2".into(),
            enabled: true,
            sync_minutes: DEFAULT_SYNC_MINUTES,
            kind: SourceKind::Caldav { url: format!("{base}/calendars/ada/work/"), username: "ada".into(), password: "x".into() },
        };
        let mut store = CalendarStore::new(vec![src], tmp("dav"));
        store.refresh_remote();
        for _ in 0..200 {
            if store.poll() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let occ = store.occurrences(today, today);
        assert_eq!(occ.len(), 1, "status: {:?}", store.status);
        assert_eq!(store.event(&occ[0]).unwrap().summary, "Remote day");
    }

    // spec: CAL-002
    #[test]
    fn unchanged_calendars_are_not_downloaded_again_and_read_only_ones_refuse_writes() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let reports = Arc::new(Mutex::new(0usize));
        let r2 = reports.clone();
        let (base, _h) = crate::cal_dav::tests::mock_http(10, move |m, _, _, _| {
            match m {
            "PROPFIND" => (207, r#"<d:multistatus xmlns:d="DAV:" xmlns:cs="http://calendarserver.org/ns/"><d:response><d:href>/ro/</d:href><d:propstat><d:prop><cs:getctag>7</cs:getctag><d:current-user-privilege-set><d:privilege><d:read/></d:privilege></d:current-user-privilege-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#.into()),
            _ => {
                *r2.lock().unwrap() += 1;
                (207, r#"<d:multistatus xmlns:d="DAV:"/>"#.into())
            }
        }
        });
        let mut store = CalendarStore::new(vec![], tmp("ctag"));
        store.set_sources(vec![dav_source("ro", format!("{base}/ro/"))]);
        let settle = |store: &mut CalendarStore| {
            for _ in 0..300 {
                store.poll();
                if !store.is_loading() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        settle(&mut store);
        assert_eq!(*reports.lock().unwrap(), 1);
        assert!(store.read_only.contains("ro"));
        assert!(store.status["ro"].contains("read-only"), "{:?}", store.status);
        store.refresh_source("ro");
        settle(&mut store);
        assert_eq!(*reports.lock().unwrap(), 1, "same ctag: not downloaded again");
        store.sync_now("ro");
        settle(&mut store);
        assert_eq!(*reports.lock().unwrap(), 2, "Sync now always downloads");
        let e = Event { uid: "x".into(), summary: "X".into(), start: 1_800_000_000, end: 1_800_001_800, ..Default::default() };
        let err = store.save_event("ro", e, None, None).unwrap_err();
        assert!(err.contains("read-only"), "{err}");
    }

    // spec: CAL-004
    #[test]
    fn a_write_keeps_the_new_version_tag() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let (base, _h) = crate::cal_dav::tests::mock_http_with(3, |m, _, _, _| match m {
            "PUT" => (204, "ETag: \"v2\"\r\n".into(), String::new()),
            _ => (404, String::new(), String::new()),
        });
        let mut store = CalendarStore::new(vec![], tmp("etag"));
        store.sources = vec![dav_source("a", format!("{base}/a/"))];
        let e = Event { uid: "u".into(), summary: "U".into(), start: 1_800_000_000, end: 1_800_001_800, href: Some(format!("{base}/a/u.ics")), etag: Some("\"v1\"".into()), ..Default::default() };
        store.events.insert("a".into(), vec![e.clone()]);
        wait(&store.save_event("a", e.clone(), Some(("a".into(), e)), None).unwrap()).unwrap();
        assert!(store.apply_fixes());
        assert_eq!(store.events["a"][0].etag.as_deref(), Some("\"v2\""));
    }

    // spec: SEC-007
    #[test]
    fn passwords_stay_out_of_debug_output() {
        let s = format!("{:?}", dav_source("a", "https://x/".into()).kind);
        assert!(!s.contains("\"x\"") && s.contains("***"), "{s}");
    }

    // spec: CAL-001, CFG-001
    #[test]
    fn config_round_trip() {
        let s = vec![
            CalendarSource { id: "a".into(), name: "Home".into(), color: "#ff0000".into(), enabled: true, sync_minutes: 15, kind: SourceKind::Local { file: "/x/a.ics".into() } },
            CalendarSource { id: "b".into(), name: "Work".into(), color: "#00ff00".into(), enabled: false, sync_minutes: 5, kind: SourceKind::Caldav { url: "https://dav".into(), username: "u".into(), password: "p".into() } },
        ];
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"type\":\"local\"") && j.contains("\"type\":\"caldav\""));
        let back: Vec<CalendarSource> = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
    }
}
