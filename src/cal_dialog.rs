//! Calendar settings dialog (local calendars, CalDAV accounts and their sync intervals) and
//! the event dialog (new events and changes, saved to any configured calendar).

use crate::cal_dav::{self, Account, FoundCalendar};
use crate::cal_ics::{self, Alarm, Event, Freq, Repeat, RepeatEnd};
use crate::cal_store::{job_result, CalendarSource, CalendarStore, Job, SourceKind, DEFAULT_SYNC_MINUTES, PALETTE};
use crate::cal_time as t;
use crate::theme;
use eframe::egui::{self, Rect, Sense, Ui, Vec2};
use std::sync::{Arc, Mutex};

type Discovery = Arc<Mutex<Option<Result<Vec<FoundCalendar>, String>>>>;

pub struct CalendarDialog {
    /// the calendars being edited (saved with "Done")
    pub sources: Vec<CalendarSource>,
    pub week_monday: bool,
    pub new_local_name: String,
    pub dav_url: String,
    pub dav_user: String,
    pub dav_pass: crate::secret::Secret,
    pub found: Vec<(FoundCalendar, bool)>,
    discovering: Option<Discovery>,
    pub message: Option<String>,
    pub rects: Vec<(String, Rect)>,
}

#[derive(Debug, PartialEq)]
pub enum CalDialogAction {
    /// save the edited list and close
    Done,
    Cancel,
    Refresh,
    /// fill this (already saved or new) local calendar with sample events
    FillSample(String),
    /// load this calendar again now
    SyncNow(String),
}

impl CalendarDialog {
    pub fn new(sources: Vec<CalendarSource>, week_monday: bool) -> Self {
        CalendarDialog { sources, week_monday, new_local_name: String::new(), dav_url: String::new(), dav_user: String::new(), dav_pass: Default::default(), found: vec![], discovering: None, message: None, rects: vec![] }
    }

    fn start_discovery(&mut self) {
        if let Err(e) = cal_dav::check_url(self.dav_url.trim()) {
            self.message = Some(format!("Can't use this address: {e}"));
            return;
        }
        let acct = Account { url: self.dav_url.trim().to_string(), username: self.dav_user.clone(), password: self.dav_pass.clone() };
        let slot: Discovery = Arc::new(Mutex::new(None));
        let s2 = slot.clone();
        crate::jobs::run(move || cal_dav::discover(&acct), move |r| *s2.lock().unwrap() = Some(r.and_then(|r| r)));
        self.discovering = Some(slot);
        self.message = Some("Looking for calendars…".into());
    }

    /// Take a finished discovery. True if it finished this time.
    pub fn poll(&mut self) -> bool {
        let Some(slot) = &self.discovering else { return false };
        let Some(r) = slot.lock().unwrap().take() else { return false };
        self.discovering = None;
        match r {
            Ok(list) => {
                self.message = Some(format!("Found {} calendar{}. Tick the ones to add.", list.len(), if list.len() == 1 { "" } else { "s" }));
                self.found = list.into_iter().map(|c| (c, true)).collect();
            }
            Err(e) => self.message = Some(format!("Could not find calendars: {e}")),
        }
        true
    }

    fn add_caldav(&mut self, url: String, name: String, color: Option<String>) {
        let id = format!("dav-{}-{}", t::now(), self.sources.len());
        let color = color.unwrap_or_else(|| PALETTE[self.sources.len() % PALETTE.len()].to_string());
        self.sources.push(CalendarSource { id, name, color, enabled: true, sync_minutes: DEFAULT_SYNC_MINUTES, kind: SourceKind::Caldav { url, username: self.dav_user.clone(), password: self.dav_pass.clone() } });
    }
}

pub fn show(ui: &mut Ui, d: &mut CalendarDialog, store: &CalendarStore) -> Option<CalDialogAction> {
    let mut rects: Vec<(String, Rect)> = Vec::new();
    let mut rec = |k: String, r: egui::Response| -> egui::Response {
        rects.push((k, r.rect));
        r
    };
    if d.poll() {
        ui.ctx().request_repaint();
    }
    if d.discovering.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
    }
    let mut action = None;
    let muted = |t: &str| egui::RichText::new(t).size(12.0).color(theme::MUTED);
    // ---- the list
    if d.sources.is_empty() {
        ui.label(muted("No calendars yet. Add a local one to try things out, or connect a CalDAV account."));
    }
    let mut remove = None;
    let mut sample = None;
    let mut sync_now = None;
    egui::Grid::new("cal-sources").num_columns(5).spacing([8.0, 6.0]).show(ui, |ui| {
        for (i, s) in d.sources.iter_mut().enumerate() {
            let (r, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
            ui.painter().circle_filled(r.center(), 7.0, crate::drawing::parse_color(&s.color).unwrap_or(theme::ACCENT));
            if rec(format!("color:{i}"), resp.on_hover_text("Click to change the colour")).clicked() {
                let pos = PALETTE.iter().position(|c| c.eq_ignore_ascii_case(&s.color)).map(|p| p + 1).unwrap_or(0);
                s.color = PALETTE[pos % PALETTE.len()].to_string();
            }
            rec(format!("enabled:{i}"), ui.checkbox(&mut s.enabled, "")).on_hover_text("Show this calendar");
            rec(format!("name:{i}"), ui.add_sized([170.0, 20.0], egui::TextEdit::singleline(&mut s.name)));
            let (kind, detail) = match &s.kind {
                SourceKind::Local { .. } => ("Local file".to_string(), String::new()),
                SourceKind::Caldav { url, .. } => ("CalDAV".to_string(), url.clone()),
            };
            let status = store.status.get(&s.id).cloned().unwrap_or_default();
            ui.label(muted(&format!("{kind}{}", if status.is_empty() { String::new() } else { format!(" · {status}") }))).on_hover_text(detail);
            ui.horizontal(|ui| {
                if s.is_local() && rec(format!("sample:{i}"), ui.small_button("Add sample events")).on_hover_text("Fill it with test events around today").clicked() {
                    sample = Some(s.id.clone());
                }
                if !s.is_local() {
                    ui.label(muted("sync every"));
                    rec(format!("sync:{i}"), ui.add(egui::DragValue::new(&mut s.sync_minutes).range(0..=1440).suffix(" min"))).on_hover_text("Load this calendar again every this many minutes (0 = only when Shale starts, or with Sync now)");
                    if rec(format!("sync-now:{i}"), ui.small_button("Sync now")).clicked() {
                        sync_now = Some(s.id.clone());
                    }
                }
                if rec(format!("remove:{i}"), ui.small_button("Remove")).clicked() {
                    remove = Some(i);
                }
            });
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        d.sources.remove(i);
    }
    if let Some(id) = sample {
        action = Some(CalDialogAction::FillSample(id));
    }
    if let Some(id) = sync_now {
        action = Some(CalDialogAction::SyncNow(id));
    }
    ui.separator();
    // ---- local
    ui.label(egui::RichText::new("Add a local calendar").strong());
    ui.horizontal(|ui| {
        rec("local-name".into(), ui.add(egui::TextEdit::singleline(&mut d.new_local_name).hint_text("Name, e.g. Test").desired_width(220.0)));
        if rec("add-local".into(), ui.button("Add")).clicked() {
            let src = store.new_local_source(&d.new_local_name, &d.sources);
            d.sources.push(src);
            d.new_local_name.clear();
        }
        if rec("add-local-sample".into(), ui.button("Add with sample events")).on_hover_text("Creates a local calendar full of test events").clicked() {
            let name = if d.new_local_name.trim().is_empty() { "Test calendar".to_string() } else { d.new_local_name.clone() };
            let src = store.new_local_source(&name, &d.sources);
            let id = src.id.clone();
            d.sources.push(src);
            d.new_local_name.clear();
            action = Some(CalDialogAction::FillSample(id));
        }
    });
    ui.label(muted("Local calendars are .ics files in your settings folder."));
    ui.separator();
    // ---- CalDAV
    ui.label(egui::RichText::new("Add CalDAV calendars").strong());
    egui::Grid::new("cal-dav").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.label(muted("Address"));
        rec("dav-url".into(), ui.add(egui::TextEdit::singleline(&mut d.dav_url).hint_text("https://cloud.example.com/remote.php/dav/").desired_width(380.0)));
        ui.end_row();
        ui.label(muted("User name"));
        rec("dav-user".into(), ui.add(egui::TextEdit::singleline(&mut d.dav_user).desired_width(220.0)));
        ui.end_row();
        ui.label(muted("Password"));
        rec("dav-pass".into(), crate::secrets::password_edit(ui, &mut d.dav_pass, "", 220.0));
        ui.end_row();
    });
    ui.horizontal(|ui| {
        let busy = d.discovering.is_some();
        if rec("dav-find".into(), ui.add_enabled(!busy && !d.dav_url.trim().is_empty(), egui::Button::new("Find calendars"))).on_hover_text("Ask the server which calendars this account has").clicked() {
            d.start_discovery();
        }
        if rec("dav-add-url".into(), ui.add_enabled(!busy && !d.dav_url.trim().is_empty(), egui::Button::new("Add this address as one calendar"))).clicked() {
            let url = d.dav_url.trim().to_string();
            match cal_dav::check_url(&url) {
                Ok(_) => {
                    let name = url.trim_end_matches('/').rsplit('/').next().unwrap_or("Calendar").to_string();
                    d.add_caldav(url, name, None);
                    d.dav_pass.clear();
                    d.message = Some("Added. Press Done to save.".into());
                }
                Err(e) => d.message = Some(format!("Can't use this address: {e}")),
            }
        }
        if busy {
            ui.spinner();
        }
    });
    if !d.found.is_empty() {
        let entered = d.dav_url.trim().to_string();
        for (i, (c, on)) in d.found.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                rec(format!("found:{i}"), ui.checkbox(on, &c.name)).on_hover_text(&c.url);
                // the password goes wherever the calendar is: say so when that's another server
                if !cal_dav::same_server(&c.url, &entered) {
                    ui.label(egui::RichText::new(format!("on {}", cal_dav::host_of(&c.url))).size(12.0).color(theme::LINK_MISSING))
                        .on_hover_text("This calendar is on another server than the address you entered; your user name and password will be sent there.");
                }
                if c.read_only {
                    ui.label(muted("read-only"));
                }
            });
        }
        if rec("dav-add-found".into(), ui.button("Add selected calendars")).clicked() {
            let picked: Vec<FoundCalendar> = d.found.iter().filter(|(_, on)| *on).map(|(c, _)| c.clone()).collect();
            for c in picked {
                d.add_caldav(c.url, c.name, c.color);
            }
            d.found.clear();
            d.dav_pass.clear();
            d.message = Some("Added. Press Done to save.".into());
        }
    }
    if let Some(m) = &d.message {
        ui.label(muted(m));
    }
    ui.label(muted(&format!("New CalDAV calendars sync every {DEFAULT_SYNC_MINUTES} minutes; change it per calendar above. Saving an event syncs its calendar right away.")));
    ui.label(muted(&format!("Only https:// addresses are accepted (http:// only for a server on this computer). {}", crate::secrets::where_stored())));
    ui.separator();
    rec("week-monday".into(), ui.checkbox(&mut d.week_monday, "Weeks start on Monday"));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if rec("done".into(), ui.button(egui::RichText::new("Done").strong())).clicked() {
            action = Some(CalDialogAction::Done);
        }
        if rec("refresh".into(), ui.button("Refresh now")).on_hover_text("Load all calendars again").clicked() {
            action = Some(CalDialogAction::Refresh);
        }
        if rec("cancel".into(), ui.button("Cancel")).clicked() {
            action = Some(CalDialogAction::Cancel);
        }
    });
    d.rects = rects;
    action
}

/// The event dialog: a new event, or changing an existing one (also moving it to another
/// calendar). It stays open until the calendar accepted the save, so an error (for example no
/// permission to write) is shown here and another calendar can be picked.
pub struct EventDialog {
    /// the calendar to save to
    pub calendar: String,
    /// editing: the calendar the event is in now, and the event as it was loaded
    pub original: Option<(String, Event)>,
    pub title: String,
    pub start_date: String,
    pub start_time: String,
    pub end_date: String,
    pub end_time: String,
    pub all_day: bool,
    pub location: String,
    pub description: String,
    pub error: Option<String>,
    /// the save that is running
    pub job: Option<Job>,
    pub rects: Vec<(String, Rect)>,
    focused: bool,
    /// the ID a new event gets (fixed when the dialog opens, so saving twice can't make two)
    new_uid: String,
    /// how the event repeats (CAL-013)
    pub repeat: RepeatMode,
    /// "every N days / weeks / …"
    pub interval: String,
    /// weekly: the days it happens on, Monday first (none: the day it starts on)
    pub days: [bool; 7],
    pub end: EndMode,
    /// the last day of the repeat, `2026-12-31`
    pub until: String,
    /// how many times it happens
    pub count: String,
    /// dates the repeat leaves out
    pub skipped: Vec<SkipRow>,
    /// the date field next to "Skip"
    pub skip_date: String,
    // the day the date pickers start their weeks on
    pub week_monday: bool,
    pub reminders: Vec<ReminderRow>,
    /// `CONFIRMED`, `TENTATIVE`, `CANCELLED`, empty for none (another value is kept)
    pub status: String,
    /// shown as free time to others (otherwise busy)
    pub free: bool,
    /// separated by commas
    pub categories: String,
    /// the named zone the times are stored in (`Europe/Berlin`); empty: UTC
    pub zone: String,
}

/// How an event repeats, as the dialog shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatMode {
    Never,
    Every(Freq),
    /// A rule made in another program that the dialog cannot show in its fields: kept as it is.
    Custom,
}

impl RepeatMode {
    pub fn label(self) -> &'static str {
        match self {
            RepeatMode::Never => "Does not repeat",
            RepeatMode::Every(Freq::Daily) => "Daily",
            RepeatMode::Every(Freq::Weekly) => "Weekly",
            RepeatMode::Every(Freq::Monthly) => "Monthly",
            RepeatMode::Every(Freq::Yearly) => "Yearly",
            RepeatMode::Custom => "As set in another program",
        }
    }
}

/// When a repeat stops, as the dialog shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndMode {
    Never,
    Until,
    Count,
}

impl EndMode {
    pub fn label(self) -> &'static str {
        match self {
            EndMode::Never => "Never",
            EndMode::Until => "On",
            EndMode::Count => "After",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Minutes,
    Hours,
    Days,
}

impl Unit {
    pub const ALL: [Unit; 3] = [Unit::Minutes, Unit::Hours, Unit::Days];
    pub fn label(self) -> &'static str {
        match self {
            Unit::Minutes => "minutes",
            Unit::Hours => "hours",
            Unit::Days => "days",
        }
    }
    fn secs(self) -> i64 {
        match self {
            Unit::Minutes => 60,
            Unit::Hours => 3600,
            Unit::Days => 86_400,
        }
    }
}

/// One reminder in the dialog: "15 minutes before".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReminderRow {
    pub amount: String,
    pub unit: Unit,
    /// the reminder of the loaded event this row shows (it is kept as it was unless the row is changed)
    original: Option<usize>,
    /// a reminder the fields cannot show (at a fixed time, after the start …): it can only be kept or removed
    pub other: bool,
}

impl ReminderRow {
    pub fn new(amount: u32, unit: Unit) -> ReminderRow {
        ReminderRow { amount: amount.to_string(), unit, original: None, other: false }
    }

    fn of(i: usize, a: &Alarm) -> ReminderRow {
        let unit = |b: i64| Unit::ALL.into_iter().rev().find(|u| b % u.secs() == 0).unwrap_or(Unit::Minutes);
        match a.before {
            Some(b) if b >= 0 && b % 60 == 0 => ReminderRow { amount: (b / unit(b).secs()).to_string(), unit: if b == 0 { Unit::Minutes } else { unit(b) }, original: Some(i), other: false },
            _ => ReminderRow { amount: String::new(), unit: Unit::Minutes, original: Some(i), other: true },
        }
    }

    fn secs(&self) -> Result<i64, String> {
        self.amount.trim().parse::<u32>().map(|n| i64::from(n) * self.unit.secs()).map_err(|_| "A reminder needs a number, such as 15 minutes before".to_string())
    }
}

/// A date the repeat leaves out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkipRow {
    /// the day shown (on this computer's clock)
    pub day: i64,
    /// the excluded start as the loaded event has it; None for a date added in the dialog
    original: Option<i64>,
}

/// Default length of a new event, in seconds.
pub const DEFAULT_EVENT_SECS: i64 = 30 * 60;

fn date_text(day: i64) -> String {
    let (y, m, d) = t::civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

fn parse_date(s: &str) -> Option<i64> {
    let mut it = s.trim().splitn(3, '-');
    let y: i32 = it.next()?.trim().parse().ok()?;
    let m: u32 = it.next()?.trim().parse().ok()?;
    let d: u32 = it.next()?.trim().parse().ok()?;
    ((1..=12).contains(&m) && d >= 1 && d <= t::days_in_month(y, m)).then(|| t::days_from_civil(y, m, d))
}

/// "9", "09", "9:30", "09:30", "0930" → seconds after midnight.
/// For tests: the time field's parser.
#[cfg(test)]
pub fn parse_time_text(s: &str) -> Option<i64> {
    parse_time(s)
}

fn parse_time(s: &str) -> Option<i64> {
    let s = s.trim();
    let (h, m) = match s.split_once(':') {
        Some((h, m)) => (h.parse::<i64>().ok()?, m.parse::<i64>().ok()?),
        None if s.len() == 4 => (s.get(..2)?.parse().ok()?, s.get(2..)?.parse().ok()?),
        None => (s.parse().ok()?, 0),
    };
    ((0..24).contains(&h) && (0..60).contains(&m)).then_some(h * 3600 + m * 60)
}

/// The day a time of an event is shown on (this computer's clock; an all-day date as it is).
fn shown_day(e: &Event, v: i64) -> i64 {
    if e.all_day {
        v.div_euclid(86_400)
    } else {
        t::utc_to_local(v).0
    }
}

/// A unique ID for a new event.
fn new_uid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("shale-{nanos:x}-{:x}-{}@shale", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

impl EventDialog {
    fn blank(calendar: String) -> Self {
        EventDialog {
            calendar,
            original: None,
            title: String::new(),
            start_date: String::new(),
            start_time: String::new(),
            end_date: String::new(),
            end_time: String::new(),
            all_day: false,
            location: String::new(),
            description: String::new(),
            error: None,
            job: None,
            rects: vec![],
            focused: false,
            new_uid: new_uid(),
            repeat: RepeatMode::Never,
            interval: "1".into(),
            days: [false; 7],
            end: EndMode::Never,
            until: String::new(),
            count: "10".into(),
            skipped: vec![],
            skip_date: String::new(),
            week_monday: true,
            reminders: vec![],
            status: String::new(),
            free: false,
            categories: String::new(),
            zone: String::new(),
        }
    }

    fn set_times(&mut self, start: i64, end: i64, all_day: bool) {
        self.all_day = all_day;
        if all_day {
            let (a, b) = (start.div_euclid(86_400), (end - 1).div_euclid(86_400));
            self.start_date = date_text(a);
            self.end_date = date_text(b.max(a));
            self.start_time = "09:00".into();
            self.end_time = "09:30".into();
        } else {
            let (sd, ss) = t::utc_to_local(start);
            let (ed, es) = t::utc_to_local(end);
            self.start_date = date_text(sd);
            self.start_time = t::hhmm(ss);
            self.end_date = date_text(ed);
            self.end_time = t::hhmm(es);
        }
    }

    /// A new event starting at `start` (UTC seconds), 30 minutes long.
    pub fn new_at(calendar: String, start: i64) -> Self {
        let mut d = Self::blank(calendar);
        d.set_times(start, start + DEFAULT_EVENT_SECS, false);
        d
    }

    /// Change an existing event of calendar `source_id`.
    pub fn edit(source_id: String, e: Event) -> Self {
        let mut d = Self::blank(source_id.clone());
        d.title = e.summary.clone();
        d.location = e.location.clone();
        d.description = e.description.clone();
        d.set_times(e.start, e.end, e.all_day);
        d.repeat = match (&e.rrule, cal_ics::parse_repeat(&e)) {
            (None, _) => RepeatMode::Never,
            (Some(_), Some(r)) => {
                d.interval = r.interval.to_string();
                d.days = r.days;
                match r.end {
                    RepeatEnd::Never => {}
                    RepeatEnd::Until(day) => {
                        d.end = EndMode::Until;
                        d.until = date_text(day);
                    }
                    RepeatEnd::Count(n) => {
                        d.end = EndMode::Count;
                        d.count = n.to_string();
                    }
                }
                RepeatMode::Every(r.freq)
            }
            (Some(_), None) => RepeatMode::Custom,
        };
        d.skipped = e.exdates.iter().map(|x| SkipRow { day: shown_day(&e, *x), original: Some(*x) }).collect();
        d.reminders = e.alarms.iter().enumerate().map(|(i, a)| ReminderRow::of(i, a)).collect();
        d.status = e.status.trim().to_ascii_uppercase();
        d.free = e.transparent;
        d.categories = e.categories.join(", ");
        d.zone = e.tzid.clone().unwrap_or_default();
        d.original = Some((source_id, e));
        d
    }

    /// Is this one date of a repeating event (changed on its own, CAL-007)?
    pub fn is_occurrence(&self) -> bool {
        self.original.as_ref().is_some_and(|(_, e)| e.recurrence_id.is_some())
    }

    /// The repeat the fields describe (None: it does not repeat, or the rule is kept as it is).
    fn repeat_of(&self, start_day: i64) -> Result<Option<Repeat>, String> {
        let RepeatMode::Every(freq) = self.repeat else { return Ok(None) };
        let interval = self.interval.trim().parse::<u32>().ok().filter(|n| *n >= 1).ok_or("“Every” needs a number, 1 or more")?;
        let end = match self.end {
            EndMode::Never => RepeatEnd::Never,
            EndMode::Until => {
                let day = parse_date(&self.until).ok_or("The last date of the repeat must look like 2026-12-31")?;
                if day < start_day {
                    return Err("The repeat ends before the event starts".into());
                }
                RepeatEnd::Until(day)
            }
            EndMode::Count => RepeatEnd::Count(self.count.trim().parse::<u32>().ok().filter(|n| *n >= 1).ok_or("“After” needs a number of times, 1 or more")?),
        };
        Ok(Some(Repeat { freq, interval, days: if freq == Freq::Weekly { self.days } else { [false; 7] }, end }))
    }

    pub fn is_saving(&self) -> bool {
        self.job.is_some()
    }

    fn start_end(&self) -> Result<(i64, i64), String> {
        let sd = parse_date(&self.start_date).ok_or("The start date must look like 2026-11-13")?;
        let ed = parse_date(&self.end_date).ok_or("The end date must look like 2026-11-13")?;
        if self.all_day {
            if ed < sd {
                return Err("The event ends before it starts".into());
            }
            return Ok((sd * 86_400, (ed + 1) * 86_400));
        }
        let st = parse_time(&self.start_time).ok_or("The start time must look like 09:30")?;
        let et = parse_time(&self.end_time).ok_or("The end time must look like 10:00")?;
        let (s, e) = (t::local_to_utc(sd, st), t::local_to_utc(ed, et));
        if e <= s {
            return Err("The event must end after it starts".into());
        }
        Ok((s, e))
    }

    /// The event described by the fields (an edited event keeps everything the dialog doesn't show).
    pub fn event(&self) -> Result<Event, String> {
        let (start, end) = self.start_end()?;
        let mut e = match &self.original {
            Some((_, e)) => e.clone(),
            None => Event { uid: self.new_uid.clone(), ..Default::default() },
        };
        e.summary = if self.title.trim().is_empty() { "New event".to_string() } else { self.title.trim().to_string() };
        e.location = self.location.trim().into();
        e.description = self.description.trim().into();
        e.start = start;
        e.end = end;
        e.all_day = self.all_day;
        let orig = self.original.as_ref().map(|(_, o)| o);
        let repeats = !self.is_occurrence() && self.repeat != RepeatMode::Never;
        // the zone the times are stored in: the one typed; a repeating event without one gets this
        // computer's, so it keeps its clock time over summer time in every program
        let zone = self.zone.trim();
        e.floating = zone.is_empty() && !self.all_day && orig.is_some_and(|o| o.floating);
        e.tzid = if self.all_day {
            None
        } else if !zone.is_empty() {
            if !t::zone_known(zone) && orig.and_then(|o| o.tzid.as_deref()) != Some(zone) {
                return Err(format!("“{zone}” is not a time zone this computer knows (it looks like Europe/Berlin); leave the field empty for UTC"));
            }
            Some(zone.to_string())
        } else if repeats && !e.floating {
            t::local_zone_name()
        } else {
            None
        };
        e.status = self.status.clone();
        e.transparent = self.free;
        e.categories = self.categories.split(',').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
        let mut alarms = Vec::new();
        for r in &self.reminders {
            let was = r.original.and_then(|i| orig.and_then(|o| o.alarms.get(i)));
            alarms.push(match was {
                Some(a) if r.other => a.clone(),
                Some(a) if Some(r.secs()?) == a.before => a.clone(),
                _ => Alarm::before_start(r.secs()?),
            });
        }
        e.alarms = alarms;
        if self.is_occurrence() {
            return Ok(e);
        }
        // the repeat: a rule that was not changed stays as it was written
        let start_day = if self.all_day { start.div_euclid(86_400) } else { t::utc_to_local(start).0 };
        match self.repeat_of(start_day)? {
            Some(r) => {
                if orig.and_then(cal_ics::parse_repeat).as_ref() != Some(&r) {
                    e.rrule = Some(cal_ics::repeat_rule(&r, &e));
                }
            }
            None if self.repeat == RepeatMode::Never => e.rrule = None,
            None => {}
        }
        // the dates left out: those of the loaded event move with its start, new ones are looked up
        let mut skipped = Vec::new();
        if repeats {
            let delta = orig.map(|o| o.wall_delta(&e)).unwrap_or(0);
            let all = Event { exdates: vec![], ..e.clone() };
            for row in &self.skipped {
                skipped.push(match (row.original, orig) {
                    (Some(x), Some(o)) => {
                        if delta == 0 {
                            x
                        } else {
                            o.shift(x, delta)
                        }
                    }
                    _ => {
                        let (from, to) = if e.all_day { (row.day * 86_400, (row.day + 1) * 86_400) } else { (t::local_to_utc(row.day, 0), t::local_to_utc(row.day + 1, 0)) };
                        let found = cal_ics::occurrences(&all, from, to).into_iter().find(|o| o.start >= from);
                        found.ok_or_else(|| format!("The event does not happen on {}, so that date cannot be skipped", date_text(row.day)))?.start
                    }
                });
            }
        }
        e.exdates = skipped;
        Ok(e)
    }

    /// Add the date in the "Skip" field to the dates the repeat leaves out.
    pub fn skip(&mut self) {
        match parse_date(&self.skip_date) {
            Some(day) => {
                if !self.skipped.iter().any(|r| r.day == day) {
                    self.skipped.push(SkipRow { day, original: None });
                    self.skipped.sort_by_key(|r| r.day);
                }
                self.skip_date.clear();
                self.error = None;
            }
            None => self.error = Some("The date to skip must look like 2026-11-13".into()),
        }
    }

    /// Check the fields and start saving. Returns false (with `error` set) if it can't start.
    pub fn save(&mut self, store: &CalendarStore, ctx: Option<egui::Context>) -> bool {
        self.error = None;
        let started = self.event().and_then(|e| store.save_event(&self.calendar, e, self.original.clone(), ctx));
        match started {
            Ok(job) => {
                self.job = Some(job);
                true
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }

    /// A finished save: Some(message) when the dialog can close, None to keep it open (an
    /// error is shown, a different calendar can be chosen and saved again).
    pub fn finish(&mut self, store: &mut CalendarStore) -> Option<String> {
        let r = job_result(self.job.as_ref()?)?;
        self.job = None;
        match r {
            Ok(msg) => {
                let mut touched = vec![self.calendar.clone()];
                if let Some((sid, _)) = &self.original {
                    touched.push(sid.clone());
                }
                store.after_write(&touched);
                Some(msg)
            }
            Err(e) => {
                // a move whose second half failed: the event is in both calendars now
                if e.contains("now in both calendars") {
                    let mut touched = vec![self.calendar.clone()];
                    if let Some((sid, _)) = &self.original {
                        touched.push(sid.clone());
                    }
                    store.after_write(&touched);
                    return Some(format!("⚠ {e}"));
                }
                self.error = Some(e);
                None
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum EventResult {
    /// the dialog is done (saved, with this message for a toast)
    Saved(String),
    Cancel,
}

/// The first day shown in the month grid: the week start on or before the 1st.
fn grid_start(y: i32, m: u32, monday_first: bool) -> i64 {
    let first = t::days_from_civil(y, m, 1);
    let back = if monday_first { t::weekday(first) } else { (t::weekday(first) + 1) % 7 };
    first - back as i64
}

/// A "📅" button that opens one month to pick the day from. Returns true when a day was picked; the text field stays typeable.
fn date_picker(ui: &mut Ui, key: &str, text: &mut String, monday_first: bool, rects: &mut Vec<(String, Rect)>) -> bool {
    let id = ui.scope_id().with(("date-picker", key));
    let mut picked = false;
    // only a click on a day closes it, so "‹" and "›" must not
    let config = egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let (button, inner) = egui::containers::menu::MenuButton::new("📅").config(config).ui(ui, |ui| {
        let sel = parse_date(text);
        let today = t::today();
        let (mut y, mut m) = ui.data(|d| d.get_temp::<(i32, u32)>(id)).unwrap_or_else(|| {
            let (y, m, _) = t::civil_from_days(sel.unwrap_or(today));
            (y, m)
        });
        let mut found: Vec<(String, Rect)> = Vec::new();
        ui.horizontal(|ui| {
            let r = ui.small_button("‹");
            found.push(("pick-prev".into(), r.rect));
            if r.clicked() {
                (y, m) = t::add_months(y, m, -1);
            }
            ui.label(egui::RichText::new(format!("{} {y}", t::MONTHS[m as usize - 1])).strong());
            let r = ui.small_button("›");
            found.push(("pick-next".into(), r.rect));
            if r.clicked() {
                (y, m) = t::add_months(y, m, 1);
            }
        });
        let start = grid_start(y, m, monday_first);
        egui::Grid::new(id.with("grid")).spacing([2.0, 2.0]).show(ui, |ui| {
            for c in 0..7 {
                let wd = (c + if monday_first { 0 } else { 6 }) % 7;
                ui.label(egui::RichText::new(t::WEEKDAYS[wd]).size(11.0).color(theme::MUTED));
            }
            ui.end_row();
            for w in 0..6 {
                for c in 0..7 {
                    let day = start + w * 7 + c;
                    let (dy, dm, dd) = t::civil_from_days(day);
                    let in_month = dm == m && dy == y;
                    let mut label = egui::RichText::new(dd.to_string());
                    if day == today {
                        label = label.strong();
                    }
                    if !in_month {
                        label = label.color(theme::MUTED);
                    }
                    let r = ui.add_sized([28.0, 20.0], egui::Button::selectable(sel == Some(day), label));
                    found.push((format!("pick-day:{}", date_text(day)), r.rect));
                    if r.clicked() {
                        *text = date_text(day);
                        picked = true;
                        ui.close();
                    }
                }
                ui.end_row();
            }
        });
        (y, m, found)
    });
    rects.push((format!("pick:{key}"), button.rect));
    if button.clicked() {
        // open on the month of the date in the field
        let (y, m, _) = t::civil_from_days(parse_date(text).unwrap_or_else(t::today));
        ui.data_mut(|d| d.insert_temp(id, (y, m)));
    }
    if let Some(inner) = inner {
        let (y, m, found) = inner.inner;
        ui.data_mut(|d| d.insert_temp(id, (y, m)));
        rects.extend(found);
    }
    picked
}

/// Show the event dialog and run its save. `store` is changed only after a finished save.
pub fn show_event(ui: &mut Ui, d: &mut EventDialog, store: &mut CalendarStore) -> Option<EventResult> {
    if let Some(msg) = d.finish(store) {
        return Some(EventResult::Saved(msg));
    }
    let mut rects: Vec<(String, Rect)> = Vec::new();
    // the pickers fill their own list, because `rec` holds `rects`
    let mut picks: Vec<(String, Rect)> = Vec::new();
    let monday_first = d.week_monday;
    let mut rec = |k: &str, r: egui::Response| -> egui::Response {
        rects.push((k.to_string(), r.rect));
        r
    };
    let mut res = None;
    let mut save = false;
    let saving = d.is_saving();
    if saving {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    let muted = |t: &str| egui::RichText::new(t).size(12.0).color(theme::MUTED);
    let before = d.start_end().ok();
    let mut start_changed = false;
    ui.add_enabled_ui(!saving, |ui| {
        egui::Grid::new("event-dialog").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label(muted("Title"));
            let r = rec("title", ui.add(egui::TextEdit::singleline(&mut d.title).hint_text("New event").desired_width(320.0)));
            if !d.focused {
                r.request_focus();
                d.focused = true;
            }
            ui.end_row();
            ui.label(muted("Save to"));
            // one date of a series stays in the calendar of its series
            let one_date = d.is_occurrence();
            let current = store.sources.iter().find(|s| s.id == d.calendar);
            let current_name = current.map(|s| s.name.clone()).unwrap_or_else(|| "(choose a calendar)".into());
            let combo = egui::ComboBox::from_id_salt("event-cal").width(320.0).selected_text(current_name).show_ui(ui, |ui| {
                for s in &store.sources {
                    let read_only = store.read_only.contains(&s.id);
                    let kind = if s.is_local() {
                        "local"
                    } else if read_only {
                        "CalDAV, read-only"
                    } else {
                        "CalDAV"
                    };
                    let label = egui::RichText::new(format!("● {}   ({kind})", s.name)).color(crate::drawing::parse_color(&s.color).unwrap_or(theme::TEXT));
                    if rec(&format!("cal:{}", s.name), ui.add_enabled((!read_only && !one_date) || d.calendar == s.id, egui::Button::selectable(d.calendar == s.id, label))).clicked() {
                        d.calendar = s.id.clone();
                        d.error = None;
                    }
                }
            });
            rec("calendar", combo.response);
            ui.end_row();
            ui.label(muted("Starts"));
            ui.horizontal(|ui| {
                let a = rec("start-date", ui.add(egui::TextEdit::singleline(&mut d.start_date).desired_width(96.0)));
                let mut b_changed = false;
                if !d.all_day {
                    let b = rec("start-time", ui.add(egui::TextEdit::singleline(&mut d.start_time).desired_width(52.0)));
                    b_changed = b.changed();
                }
                let picked = date_picker(ui, "start-date", &mut d.start_date, monday_first, &mut picks);
                start_changed = a.changed() || b_changed || picked;
                // one date of a series keeps the kind of time its series has
                rec("all-day", ui.add_enabled(!one_date, egui::Checkbox::new(&mut d.all_day, "All day")));
            });
            ui.end_row();
            ui.label(muted("Ends"));
            ui.horizontal(|ui| {
                rec("end-date", ui.add(egui::TextEdit::singleline(&mut d.end_date).desired_width(96.0)));
                date_picker(ui, "end-date", &mut d.end_date, monday_first, &mut picks);
                if !d.all_day {
                    rec("end-time", ui.add(egui::TextEdit::singleline(&mut d.end_time).desired_width(52.0)));
                }
            });
            ui.end_row();
            ui.label(muted("Where"));
            rec("location", ui.add(egui::TextEdit::singleline(&mut d.location).desired_width(320.0)));
            ui.end_row();
            ui.label(muted("Notes"));
            rec("description", ui.add(egui::TextEdit::multiline(&mut d.description).desired_rows(3).desired_width(320.0)));
            ui.end_row();
            ui.label(muted("Repeats"));
            if d.is_occurrence() {
                ui.label(muted("One date of a repeating event: only this date changes."));
            } else {
                ui.horizontal(|ui| {
                    let combo = egui::ComboBox::from_id_salt("event-repeat").width(150.0).selected_text(d.repeat.label()).show_ui(ui, |ui| {
                        let was_custom = d.original.as_ref().is_some_and(|(_, e)| e.rrule.is_some() && cal_ics::parse_repeat(e).is_none());
                        let mut modes = vec![RepeatMode::Never];
                        modes.extend(Freq::ALL.map(RepeatMode::Every));
                        if was_custom {
                            modes.push(RepeatMode::Custom);
                        }
                        for m in modes {
                            if rec(&format!("repeat:{}", m.label()), ui.selectable_label(d.repeat == m, m.label())).clicked() {
                                d.repeat = m;
                            }
                        }
                    });
                    rec("repeat", combo.response);
                    if let RepeatMode::Every(f) = d.repeat {
                        ui.label(muted("every"));
                        rec("interval", ui.add(egui::TextEdit::singleline(&mut d.interval).desired_width(28.0)));
                        ui.label(muted(match f {
                            Freq::Daily => "day(s)",
                            Freq::Weekly => "week(s)",
                            Freq::Monthly => "month(s)",
                            Freq::Yearly => "year(s)",
                        }));
                    }
                });
            }
            ui.end_row();
            if !d.is_occurrence() && d.repeat == RepeatMode::Custom {
                ui.label(muted(""));
                let rule = d.original.as_ref().and_then(|(_, e)| e.rrule.clone()).unwrap_or_default();
                ui.add(egui::Label::new(muted(&format!("{rule} — kept as it is. Choose another repeat above to replace it."))).wrap());
                ui.end_row();
            }
            if !d.is_occurrence() && d.repeat == RepeatMode::Every(Freq::Weekly) {
                ui.label(muted("On"));
                ui.horizontal(|ui| {
                    for (i, name) in t::WEEKDAYS.iter().enumerate() {
                        if rec(&format!("day:{i}"), ui.selectable_label(d.days[i], *name)).clicked() {
                            d.days[i] = !d.days[i];
                        }
                    }
                });
                ui.end_row();
            }
            if !d.is_occurrence() && matches!(d.repeat, RepeatMode::Every(_)) {
                ui.label(muted("Ends"));
                ui.horizontal(|ui| {
                    let combo = egui::ComboBox::from_id_salt("event-end").width(80.0).selected_text(d.end.label()).show_ui(ui, |ui| {
                        for m in [EndMode::Never, EndMode::Until, EndMode::Count] {
                            if rec(&format!("end:{}", m.label()), ui.selectable_label(d.end == m, m.label())).clicked() {
                                d.end = m;
                                if m == EndMode::Until && d.until.trim().is_empty() {
                                    d.until = d.end_date.clone();
                                }
                            }
                        }
                    });
                    rec("end", combo.response);
                    match d.end {
                        EndMode::Never => {}
                        EndMode::Until => {
                            rec("until", ui.add(egui::TextEdit::singleline(&mut d.until).hint_text("2026-12-31").desired_width(96.0)));
                            date_picker(ui, "until", &mut d.until, monday_first, &mut picks);
                        }
                        EndMode::Count => {
                            rec("count", ui.add(egui::TextEdit::singleline(&mut d.count).desired_width(36.0)));
                            ui.label(muted("times"));
                        }
                    }
                });
                ui.end_row();
            }
            if !d.is_occurrence() && d.repeat != RepeatMode::Never {
                ui.label(muted("Skips"));
                ui.vertical(|ui| {
                    let mut remove = None;
                    ui.horizontal_wrapped(|ui| {
                        for (i, row) in d.skipped.iter().enumerate() {
                            if rec(&format!("unskip:{i}"), ui.button(format!("{} ✕", date_text(row.day)))).on_hover_text("This date happens again").clicked() {
                                remove = Some(i);
                            }
                        }
                    });
                    if let Some(i) = remove {
                        d.skipped.remove(i);
                    }
                    ui.horizontal(|ui| {
                        let r = rec("skip-date", ui.add(egui::TextEdit::singleline(&mut d.skip_date).hint_text("2026-11-13").desired_width(96.0)));
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if date_picker(ui, "skip-date", &mut d.skip_date, monday_first, &mut picks) {
                            d.skip();
                        }
                        if rec("skip-add", ui.button("Skip this date")).on_hover_text("The event does not happen on that date").clicked() || enter {
                            d.skip();
                        }
                    });
                });
                ui.end_row();
            }
            ui.label(muted("Reminders"));
            ui.vertical(|ui| {
                let mut remove = None;
                for (i, row) in d.reminders.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        if row.other {
                            ui.label(muted("A reminder set in another program (kept as it is)"));
                        } else {
                            rec(&format!("rem-amount:{i}"), ui.add(egui::TextEdit::singleline(&mut row.amount).desired_width(36.0)));
                            let combo = egui::ComboBox::from_id_salt(("event-rem", i)).width(80.0).selected_text(row.unit.label()).show_ui(ui, |ui| {
                                for u in Unit::ALL {
                                    if rec(&format!("rem-unit:{i}:{}", u.label()), ui.selectable_label(row.unit == u, u.label())).clicked() {
                                        row.unit = u;
                                    }
                                }
                            });
                            rec(&format!("rem-unit:{i}"), combo.response);
                            ui.label(muted("before"));
                        }
                        if rec(&format!("rem-del:{i}"), ui.small_button("✕")).on_hover_text("Remove this reminder").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    d.reminders.remove(i);
                }
                if rec("rem-add", ui.button("＋ Add a reminder")).clicked() {
                    d.reminders.push(ReminderRow::new(15, Unit::Minutes));
                }
            });
            ui.end_row();
            ui.label(muted("Status"));
            ui.horizontal(|ui| {
                let known = [("", "None"), ("CONFIRMED", "Confirmed"), ("TENTATIVE", "Tentative"), ("CANCELLED", "Cancelled")];
                let shown = known.iter().find(|k| k.0 == d.status).map(|k| k.1.to_string()).unwrap_or_else(|| d.status.to_lowercase());
                let combo = egui::ComboBox::from_id_salt("event-status").width(110.0).selected_text(shown).show_ui(ui, |ui| {
                    for (value, label) in known {
                        if rec(&format!("status:{label}"), ui.selectable_label(d.status == value, label)).clicked() {
                            d.status = value.to_string();
                        }
                    }
                });
                rec("status", combo.response);
                ui.label(muted("Show as"));
                let combo = egui::ComboBox::from_id_salt("event-busy").width(70.0).selected_text(if d.free { "Free" } else { "Busy" }).show_ui(ui, |ui| {
                    for (free, label) in [(false, "Busy"), (true, "Free")] {
                        if rec(&format!("show-as:{label}"), ui.selectable_label(d.free == free, label)).clicked() {
                            d.free = free;
                        }
                    }
                });
                rec("show-as", combo.response);
            });
            ui.end_row();
            ui.label(muted("Categories"));
            rec("categories", ui.add(egui::TextEdit::singleline(&mut d.categories).hint_text("Work, Travel").desired_width(320.0)));
            ui.end_row();
            if !d.all_day {
                ui.label(muted("Time zone"));
                let hint = t::local_zone_name().map(|z| format!("UTC (a repeating event: {z})")).unwrap_or_else(|| "UTC".into());
                rec("zone", ui.add(egui::TextEdit::singleline(&mut d.zone).hint_text(hint).desired_width(320.0))).on_hover_text("The zone the times are stored in, such as Europe/Berlin. The fields above always show this computer's time.");
                ui.end_row();
            }
        });
    });
    // moving the start keeps the length of the event
    if start_changed && !d.all_day {
        if let (Some((s0, e0)), Some(sd), Some(st)) = (before, parse_date(&d.start_date), parse_time(&d.start_time)) {
            let s = t::local_to_utc(sd, st);
            let (ed, es) = t::utc_to_local(s + (e0 - s0));
            d.end_date = date_text(ed);
            d.end_time = t::hhmm(es);
        }
    }
    if let Some((_, e)) = &d.original {
        if e.rrule.is_some() {
            ui.label(muted("This event repeats: changes apply to every repeat. To change one date only, close this and choose “Edit this date”."));
        }
    }
    if let Some(e) = &d.error {
        ui.add(egui::Label::new(egui::RichText::new(e).color(theme::LINK_MISSING)).wrap());
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let label = if d.original.as_ref().map(|(s, _)| *s != d.calendar).unwrap_or(false) { "Move here" } else { "Save" };
        if rec("save", ui.add_enabled(!saving, egui::Button::new(egui::RichText::new(label).strong()))).clicked() {
            save = true;
        }
        if rec("cancel", ui.add_enabled(!saving, egui::Button::new("Cancel"))).clicked() {
            res = Some(EventResult::Cancel);
        }
        if saving {
            ui.spinner();
            let name = store.sources.iter().find(|s| s.id == d.calendar).map(|s| s.name.clone()).unwrap_or_default();
            ui.label(muted(&format!("Saving to “{name}”…")));
        }
    });
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
    rects.extend(picks);
    d.rects = rects;
    if (save || enter) && !saving {
        d.save(store, Some(ui.ctx().clone()));
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event as Ev, Modifiers, PointerButton, Pos2, RawInput};

    struct H {
        ctx: egui::Context,
        t: f64,
    }
    impl H {
        fn frame<R>(&mut self, events: Vec<Ev>, mut f: impl FnMut(&mut Ui) -> Option<R>) -> Option<R> {
            self.t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 1000.0))), time: Some(self.t), events, ..Default::default() };
            let mut res = None;
            let mut out = self.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    if let Some(r) = f(ui) {
                        res = Some(r);
                    }
                });
            });
            out.textures_delta.clear();
            res
        }
        fn click<R>(&mut self, rect: Rect, mut f: impl FnMut(&mut Ui) -> Option<R>) -> Option<R> {
            let p = rect.center();
            let b = |pressed| Ev::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            let mut r = self.frame(vec![Ev::PointerMoved(p)], &mut f);
            r = self.frame(vec![b(true)], &mut f).or(r);
            r = self.frame(vec![b(false)], &mut f).or(r);
            self.frame(vec![], &mut f).or(r)
        }
        fn typ<R>(&mut self, rect: Rect, text: &str, mut f: impl FnMut(&mut Ui) -> Option<R>) {
            self.click(rect, &mut f);
            self.frame(vec![Ev::Text(text.into())], &mut f);
            self.frame(vec![], &mut f);
        }
    }

    fn rect(rects: &[(String, Rect)], key: &str) -> Rect {
        rects.iter().find(|r| r.0 == key).map(|r| r.1).unwrap_or_else(|| panic!("no {key}: {:?}", rects.iter().map(|r| &r.0).collect::<Vec<_>>()))
    }

    // spec: CAL-010
    #[test]
    fn calendar_dialog_controls() {
        let store = CalendarStore::new(vec![], std::env::temp_dir().join(format!("shale-caldlg-{}", std::process::id())));
        let mut d = CalendarDialog::new(vec![], true);
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        h.frame(vec![], |ui| show(ui, &mut d, &store));
        h.typ(rect(&d.rects, "local-name"), "Home", |ui| show(ui, &mut d, &store));
        assert_eq!(d.new_local_name, "Home");
        h.click(rect(&d.rects, "add-local"), |ui| show(ui, &mut d, &store));
        assert_eq!(d.sources.len(), 1);
        assert_eq!(d.sources[0].name, "Home");
        assert!(d.sources[0].is_local());
        let a = h.click(rect(&d.rects, "add-local-sample"), |ui| show(ui, &mut d, &store));
        assert_eq!(d.sources.len(), 2);
        assert!(matches!(a, Some(CalDialogAction::FillSample(id)) if id == d.sources[1].id));
        let a = h.click(rect(&d.rects, "sample:0"), |ui| show(ui, &mut d, &store));
        assert!(matches!(a, Some(CalDialogAction::FillSample(id)) if id == d.sources[0].id));
        let c = d.sources[0].color.clone();
        h.click(rect(&d.rects, "color:0"), |ui| show(ui, &mut d, &store));
        assert_ne!(d.sources[0].color, c, "colour button");
        h.click(rect(&d.rects, "enabled:0"), |ui| show(ui, &mut d, &store));
        assert!(!d.sources[0].enabled, "show/hide checkbox");
        h.click(rect(&d.rects, "remove:1"), |ui| show(ui, &mut d, &store));
        assert_eq!(d.sources.len(), 1, "remove button");
        // CalDAV fields and adding an address
        h.typ(rect(&d.rects, "dav-url"), "https://dav.example.com/cal/work/", |ui| show(ui, &mut d, &store));
        h.typ(rect(&d.rects, "dav-user"), "ada", |ui| show(ui, &mut d, &store));
        h.typ(rect(&d.rects, "dav-pass"), "secret", |ui| show(ui, &mut d, &store));
        h.click(rect(&d.rects, "dav-add-url"), |ui| show(ui, &mut d, &store));
        assert_eq!(d.sources.len(), 2);
        assert_eq!(d.sources[1].kind, SourceKind::Caldav { url: "https://dav.example.com/cal/work/".into(), username: "ada".into(), password: "secret".into() });
        assert_eq!(d.sources[1].name, "work");
        assert_eq!(d.sources[1].sync_minutes, 15, "auto-sync every 15 minutes by default");
        assert!(!d.rects.iter().any(|r| r.0 == "sync:0"), "local calendars have no sync timer");
        h.frame(vec![], |ui| show(ui, &mut d, &store));
        let r = rect(&d.rects, "sync:1");
        h.click(r, |ui| show(ui, &mut d, &store));
        h.frame(vec![Ev::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }], |ui| show(ui, &mut d, &store));
        h.frame(vec![Ev::Text("5".into())], |ui| show(ui, &mut d, &store));
        h.frame(vec![Ev::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }], |ui| show(ui, &mut d, &store));
        h.frame(vec![], |ui| show(ui, &mut d, &store));
        assert_eq!(d.sources[1].sync_minutes, 5, "the sync interval can be changed");
        let id = d.sources[1].id.clone();
        assert_eq!(h.click(rect(&d.rects, "sync-now:1"), |ui| show(ui, &mut d, &store)), Some(CalDialogAction::SyncNow(id)));
        h.click(rect(&d.rects, "week-monday"), |ui| show(ui, &mut d, &store));
        assert!(!d.week_monday);
        assert_eq!(h.click(rect(&d.rects, "refresh"), |ui| show(ui, &mut d, &store)), Some(CalDialogAction::Refresh));
        assert_eq!(h.click(rect(&d.rects, "cancel"), |ui| show(ui, &mut d, &store)), Some(CalDialogAction::Cancel));
        assert_eq!(h.click(rect(&d.rects, "done"), |ui| show(ui, &mut d, &store)), Some(CalDialogAction::Done));
    }

    // spec: CAL-010
    #[test]
    fn find_calendars_button_uses_the_server() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        let (base, _h) = crate::cal_dav::tests::mock_server(String::new());
        let store = CalendarStore::new(vec![], std::env::temp_dir().join("shale-caldlg-find"));
        let mut d = CalendarDialog::new(vec![], true);
        d.dav_url = format!("{base}/");
        d.dav_user = "ada".into();
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        h.frame(vec![], |ui| show(ui, &mut d, &store));
        h.click(rect(&d.rects, "dav-find"), |ui| show(ui, &mut d, &store));
        for _ in 0..200 {
            h.frame(vec![], |ui| show(ui, &mut d, &store));
            if !d.found.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(d.found.len(), 1, "message: {:?}", d.message);
        h.frame(vec![], |ui| show(ui, &mut d, &store));
        h.click(rect(&d.rects, "dav-add-found"), |ui| show(ui, &mut d, &store));
        assert_eq!(d.sources.len(), 1);
        assert_eq!(d.sources[0].name, "Work");
        assert_eq!(d.sources[0].color, "#1971c2");
    }

    fn wait_saved(h: &mut H, d: &mut EventDialog, store: &mut CalendarStore) -> Option<EventResult> {
        for _ in 0..300 {
            if let Some(r) = h.frame(vec![], |ui| show_event(ui, d, store)) {
                return Some(r);
            }
            if !d.is_saving() {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the save did not finish")
    }

    // spec: CAL-005, CAL-008
    #[test]
    fn event_dialog_fields() {
        let dir = std::env::temp_dir().join(format!("shale-evdlg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = CalendarStore::new(vec![], dir);
        let src = store.new_local_source("Home", &[]);
        store.set_sources(vec![src.clone()]);
        let day = t::days_from_civil(2026, 11, 13);
        let start = t::local_to_utc(day, 9 * 3600 + 30 * 60);
        let mut d = EventDialog::new_at(src.id.clone(), start);
        assert_eq!((d.start_date.as_str(), d.start_time.as_str(), d.end_date.as_str(), d.end_time.as_str()), ("2026-11-13", "09:30", "2026-11-13", "10:00"), "30 minutes by default");
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        h.frame(vec![Ev::Text("Dentist".into())], |ui| show_event(ui, &mut d, &mut store));
        assert_eq!(d.title, "Dentist", "the title field has focus first");
        h.typ(rect(&d.rects, "location"), "Main street 4", |ui| show_event(ui, &mut d, &mut store));
        let e = d.event().unwrap();
        assert_eq!((e.summary.as_str(), e.location.as_str(), e.end - e.start), ("Dentist", "Main street 4", 1800));
        assert_eq!(t::utc_to_local(e.start), (day, 9 * 3600 + 1800));
        // a later start moves the end along (same length)
        h.click(rect(&d.rects, "start-time"), |ui| show_event(ui, &mut d, &mut store));
        h.frame(vec![Ev::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }], |ui| show_event(ui, &mut d, &mut store));
        h.frame(vec![Ev::Text("14:00".into())], |ui| show_event(ui, &mut d, &mut store));
        assert_eq!((d.start_time.as_str(), d.end_time.as_str()), ("14:00", "14:30"));
        // the end can be set freely, also on another day
        d.end_date = "2026-11-14".into();
        d.end_time = "1".into();
        assert_eq!(d.event().unwrap().end - d.event().unwrap().start, 11 * 3600);
        d.end_date = "2026-11-13".into();
        d.end_time = "13:00".into();
        assert!(d.event().unwrap_err().contains("end after"), "an end before the start is refused");
        d.end_time = "15:00".into();
        h.click(rect(&d.rects, "all-day"), |ui| show_event(ui, &mut d, &mut store));
        let e = d.event().unwrap();
        assert!(e.all_day);
        assert_eq!((e.start, e.end), (day * 86_400, (day + 1) * 86_400));
        d.start_date = "13.11.2026".into();
        assert!(d.event().is_err(), "a bad date is reported");
        d.start_date = "2026-11-13".into();
        d.all_day = false;
        d.start_time = "25:00".into();
        assert!(d.event().is_err(), "a bad time is reported");
        d.start_time = "14:00".into();
        // Save writes it, then the dialog closes
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        // a fast job can finish during the click: its result comes back from the click
        let clicked = h.click(rect(&d.rects, "save"), |ui| show_event(ui, &mut d, &mut store));
        let r = clicked.or_else(|| wait_saved(&mut h, &mut d, &mut store));
        assert!(matches!(&r, Some(EventResult::Saved(m)) if m.starts_with("Saved")), "{r:?} {:?}", d.error);
        let saved = store.find(&src.id, &e.uid).expect("the event is in the calendar");
        assert_eq!(saved.summary, "Dentist");
        // editing keeps the event's ID and changes it in place
        let mut d = EventDialog::edit(src.id.clone(), saved.clone());
        assert_eq!((d.title.as_str(), d.start_time.as_str(), d.end_time.as_str()), ("Dentist", "14:00", "15:00"));
        d.title = "Dentist (moved)".into();
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        // a fast job can finish during the click: its result comes back from the click
        let clicked = h.click(rect(&d.rects, "save"), |ui| show_event(ui, &mut d, &mut store));
        clicked.or_else(|| wait_saved(&mut h, &mut d, &mut store)).unwrap();
        assert_eq!(store.find(&src.id, &saved.uid).unwrap().summary, "Dentist (moved)");
        assert_eq!(store.events[&src.id].iter().filter(|x| x.uid == saved.uid).count(), 1, "replaced, not copied");
        let mut d = EventDialog::new_at(src.id.clone(), start);
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        assert_eq!(h.click(rect(&d.rects, "cancel"), |ui| show_event(ui, &mut d, &mut store)), Some(EventResult::Cancel));
    }

    // spec: CAL-004
    #[test]
    fn event_dialog_shows_write_errors_and_saves_elsewhere() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            return;
        }
        // "Shared" refuses writes, "Work" accepts them
        let (base, _h) = crate::cal_dav::tests::mock_http(40, |method, path, _, _| match (method, path) {
            ("PUT", p) if p.starts_with("/shared/") => (403, String::new()),
            ("PUT", _) => (201, String::new()),
            ("DELETE", _) => (204, String::new()),
            _ => (404, String::new()),
        });
        let dir = std::env::temp_dir().join(format!("shale-evdlg-dav-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = CalendarStore::new(vec![], dir);
        let local = store.new_local_source("Home", &[]);
        let dav = |id: &str, name: &str| CalendarSource {
            id: id.into(),
            name: name.into(),
            color: "#1971c2".into(),
            enabled: true,
            sync_minutes: 0,
            kind: SourceKind::Caldav { url: format!("{base}/{id}/"), username: "ada".into(), password: "x".into() },
        };
        store.set_sources(vec![local.clone(), dav("shared", "Shared"), dav("work", "Work")]);
        let mut d = EventDialog::new_at("shared".into(), 1_800_000_000);
        d.title = "Kickoff".into();
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        // a fast job can finish during the click: its result comes back from the click
        let clicked = h.click(rect(&d.rects, "save"), |ui| show_event(ui, &mut d, &mut store));
        assert!(clicked.or_else(|| wait_saved(&mut h, &mut d, &mut store)).is_none(), "the dialog stays open");
        assert!(d.error.as_deref().unwrap_or("").contains("permission"), "{:?}", d.error);
        // pick another calendar and save again
        h.click(rect(&d.rects, "calendar"), |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "cal:Work"), |ui| show_event(ui, &mut d, &mut store));
        assert_eq!(d.calendar, "work");
        assert!(d.error.is_none());
        // a fast job can finish during the click: its result comes back from the click
        let clicked = h.click(rect(&d.rects, "save"), |ui| show_event(ui, &mut d, &mut store));
        let r = clicked.or_else(|| wait_saved(&mut h, &mut d, &mut store));
        assert!(matches!(&r, Some(EventResult::Saved(m)) if m.contains("Work")), "{r:?} {:?}", d.error);
        // editing a local event and changing "Save to": written to the server, then removed here
        let e = Event { uid: "loc1".into(), summary: "Haircut".into(), start: 1_800_000_000, end: 1_800_001_800, ..Default::default() };
        let mut d = EventDialog::new_at(local.id.clone(), 0);
        d.title = e.summary.clone();
        d.set_times(e.start, e.end, false);
        let mut fixed = d.event().unwrap();
        fixed.uid = "loc1".into();
        d.original = None;
        crate::cal_store::tests::wait(&store.save_event(&local.id, fixed.clone(), None, None).unwrap()).unwrap();
        store.reload_local();
        let mut d = EventDialog::edit(local.id.clone(), store.find(&local.id, "loc1").unwrap());
        d.calendar = "work".into();
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        // a fast job can finish during the click: its result comes back from the click
        let clicked = h.click(rect(&d.rects, "save"), |ui| show_event(ui, &mut d, &mut store));
        let r = clicked.or_else(|| wait_saved(&mut h, &mut d, &mut store));
        assert!(matches!(&r, Some(EventResult::Saved(m)) if m.starts_with("Moved")), "{r:?} {:?}", d.error);
        assert!(store.find(&local.id, "loc1").is_none(), "removed from the old calendar after the new one accepted it");
    }

    // spec: CAL-013, CAL-007
    #[test]
    fn event_dialog_repeats_skipped_dates_reminders_and_the_other_fields() {
        let dir = std::env::temp_dir().join(format!("shale-evdlg-series-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = CalendarStore::new(vec![], dir);
        let src = store.new_local_source("Home", &[]);
        store.set_sources(vec![src.clone()]);
        let day = t::days_from_civil(2026, 11, 13); // a Friday
        let mut d = EventDialog::new_at(src.id.clone(), t::local_to_utc(day, 9 * 3600));
        d.title = "Standup".into();
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        assert!(!d.rects.iter().any(|r| r.0 == "interval" || r.0 == "end" || r.0 == "skip-add"), "no repeat fields while it does not repeat");
        // weekly on Monday and Wednesday, 10 times
        h.click(rect(&d.rects, "repeat"), |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "repeat:Weekly"), |ui| show_event(ui, &mut d, &mut store));
        assert_eq!(d.repeat, RepeatMode::Every(Freq::Weekly));
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "day:0"), |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "day:2"), |ui| show_event(ui, &mut d, &mut store));
        assert_eq!(d.days, [true, false, true, false, false, false, false]);
        h.click(rect(&d.rects, "end"), |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "end:After"), |ui| show_event(ui, &mut d, &mut store));
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        assert!(d.rects.iter().any(|r| r.0 == "count"));
        // a reminder (15 minutes by default), changed to 2 hours; a second one removed again
        h.click(rect(&d.rects, "rem-add"), |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "rem-add"), |ui| show_event(ui, &mut d, &mut store));
        assert_eq!(d.reminders.len(), 2);
        h.click(rect(&d.rects, "rem-del:1"), |ui| show_event(ui, &mut d, &mut store));
        assert_eq!(d.reminders, vec![ReminderRow::new(15, Unit::Minutes)]);
        h.click(rect(&d.rects, "rem-unit:0"), |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "rem-unit:0:hours"), |ui| show_event(ui, &mut d, &mut store));
        d.reminders[0].amount = "2".into();
        // status, free or busy, categories
        h.click(rect(&d.rects, "status"), |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "status:Tentative"), |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "show-as"), |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "show-as:Free"), |ui| show_event(ui, &mut d, &mut store));
        h.typ(rect(&d.rects, "categories"), "Work, Team", |ui| show_event(ui, &mut d, &mut store));
        let e = d.event().unwrap();
        assert_eq!(e.rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=MO,WE;COUNT=10"));
        assert_eq!((e.status.as_str(), e.transparent, e.categories.clone()), ("TENTATIVE", true, vec!["Work".to_string(), "Team".to_string()]));
        assert_eq!(e.alarms, vec![Alarm::before_start(7200)]);
        assert_eq!(e.tzid, t::local_zone_name(), "a repeating event is stored in this computer's zone, so it keeps its clock time");
        // skip a date: only a date the event happens on
        h.typ(rect(&d.rects, "skip-date"), "2026-11-18", |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "skip-add"), |ui| show_event(ui, &mut d, &mut store));
        assert_eq!(d.skipped.iter().map(|r| r.day).collect::<Vec<_>>(), vec![day + 5]);
        let e = d.event().unwrap();
        assert_eq!(e.exdates, vec![t::local_to_utc(day + 5, 9 * 3600)], "the Wednesday at 09:00");
        d.skip_date = "2026-11-17".into();
        d.skip();
        assert!(d.event().unwrap_err().contains("does not happen on 2026-11-17"));
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "unskip:0"), |ui| show_event(ui, &mut d, &mut store));
        assert_eq!(d.skipped.len(), 1, "the wrong date is taken out again");
        d.skip_date = "17.11.".into();
        d.skip();
        assert!(d.error.as_deref().unwrap_or("").contains("2026-11-13"), "{:?}", d.error);
        // bad values are said, nothing is saved
        d.interval = "0".into();
        assert!(d.event().unwrap_err().contains("Every"));
        d.interval = "1".into();
        d.reminders[0].amount = "soon".into();
        assert!(d.event().unwrap_err().contains("reminder"));
        d.reminders[0].amount = "2".into();
        d.zone = "Nowhere/Land".into();
        assert!(d.event().unwrap_err().contains("time zone"));
        d.zone.clear();
        d.end = EndMode::Until;
        d.until = "2026-11-01".into();
        assert!(d.event().unwrap_err().contains("ends before"));
        d.end = EndMode::Count;
        // saved, loaded again: the dialog shows the same, and saving without a change changes nothing
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        // a fast job can finish during the click: its result comes back from the click
        let clicked = h.click(rect(&d.rects, "save"), |ui| show_event(ui, &mut d, &mut store));
        assert!(clicked.or_else(|| wait_saved(&mut h, &mut d, &mut store)).is_some(), "{:?}", d.error);
        let saved = store.find(&src.id, &e.uid).unwrap();
        let mut d = EventDialog::edit(src.id.clone(), saved.clone());
        assert_eq!((d.repeat, d.interval.as_str(), d.days, d.end, d.count.as_str()), (RepeatMode::Every(Freq::Weekly), "1", [true, false, true, false, false, false, false], EndMode::Count, "10"));
        assert_eq!((d.reminders[0].amount.as_str(), d.reminders[0].unit, d.status.as_str(), d.free, d.categories.as_str()), ("2", Unit::Hours, "TENTATIVE", true, "Work, Team"));
        assert_eq!(d.skipped.iter().map(|r| r.day).collect::<Vec<_>>(), vec![day + 5]);
        assert_eq!(d.event().unwrap(), saved, "nothing changes by opening and saving");
        // an hour later: the skipped date moves with the series
        d.start_time = "10:00".into();
        d.end_time = "10:30".into();
        assert_eq!(d.event().unwrap().exdates, vec![t::local_to_utc(day + 5, 10 * 3600)]);
        // "does not repeat": the rule and the skipped dates go
        d.repeat = RepeatMode::Never;
        let single = d.event().unwrap();
        assert!(single.rrule.is_none() && single.exdates.is_empty());
        // a rule from another program that has no fields here is shown and kept
        let other = Event { rrule: Some("FREQ=MONTHLY;BYDAY=2TU".into()), ..saved.clone() };
        let mut d = EventDialog::edit(src.id.clone(), other.clone());
        assert_eq!(d.repeat, RepeatMode::Custom);
        d.title = "Renamed".into();
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        assert!(!d.rects.iter().any(|r| r.0 == "interval"));
        let e = d.event().unwrap();
        assert_eq!((e.rrule.as_deref(), e.exdates.clone()), (Some("FREQ=MONTHLY;BYDAY=2TU"), other.exdates.clone()));
        // one date of the series: no repeat fields, the calendar and "all day" are fixed, and the save adds it to the series
        let occ = cal_ics::occurrences(&saved, 0, i64::MAX / 2)[0].clone();
        let mut d = EventDialog::edit(src.id.clone(), saved.override_at(occ.start, occ.end));
        assert!(d.is_occurrence());
        d.title = "Standup (short)".into();
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        assert!(!d.rects.iter().any(|r| r.0 == "repeat" || r.0 == "skip-add"), "one date has no repeat of its own");
        let one = d.event().unwrap();
        assert_eq!((one.recurrence_id, one.rrule.clone(), one.alarms.len()), (Some(occ.start), None, 1));
        // a fast job can finish during the click: its result comes back from the click
        let clicked = h.click(rect(&d.rects, "save"), |ui| show_event(ui, &mut d, &mut store));
        assert!(clicked.or_else(|| wait_saved(&mut h, &mut d, &mut store)).is_some(), "{:?}", d.error);
        let parts: Vec<&Event> = store.events[&src.id].iter().filter(|x| x.uid == saved.uid).collect();
        assert_eq!(parts.len(), 2, "the series and its changed date");
        assert!(parts.iter().any(|x| x.rrule.is_some() && x.summary == "Standup") && parts.iter().any(|x| x.recurrence_id == Some(occ.start) && x.summary == "Standup (short)"));
        let shown: Vec<String> = store.occurrences(day, day + 6).iter().filter_map(|o| store.event(o)).map(|x| x.summary.clone()).collect();
        assert_eq!(shown, vec!["Standup (short)"], "Monday is the changed date, Wednesday is skipped");
    }

    // spec: CAL-015
    #[test]
    fn date_fields_have_a_month_to_pick_from() {
        let dir = std::env::temp_dir().join(format!("shale-evdlg-pick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = CalendarStore::new(vec![], dir);
        let src = store.new_local_source("Home", &[]);
        store.set_sources(vec![src.clone()]);
        let day = t::days_from_civil(2026, 11, 13);
        let mut d = EventDialog::new_at(src.id.clone(), t::local_to_utc(day, 9 * 3600));
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        let has_days = |d: &EventDialog| d.rects.iter().any(|r| r.0.starts_with("pick-day:"));
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        assert!(!has_days(&d), "closed at first");
        // start: a month on, a day; the end moves along
        h.click(rect(&d.rects, "pick:start-date"), |ui| show_event(ui, &mut d, &mut store));
        assert!(rect(&d.rects, "pick-day:2026-11-13").width() > 0.0, "opens on the month of the field");
        h.click(rect(&d.rects, "pick-prev"), |ui| show_event(ui, &mut d, &mut store));
        assert!(has_days(&d), "going back keeps it open");
        assert!(d.rects.iter().any(|r| r.0 == "pick-day:2026-10-15"));
        h.click(rect(&d.rects, "pick-next"), |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "pick-next"), |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "pick-day:2026-12-20"), |ui| show_event(ui, &mut d, &mut store));
        assert_eq!((d.start_date.as_str(), d.start_time.as_str(), d.end_date.as_str(), d.end_time.as_str()), ("2026-12-20", "09:00", "2026-12-20", "09:30"), "same length");
        assert!(!has_days(&d), "a pick closes it");
        // end
        h.click(rect(&d.rects, "pick:end-date"), |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "pick-day:2026-12-22"), |ui| show_event(ui, &mut d, &mut store));
        assert_eq!(d.end_date, "2026-12-22");
        // skip, with a weekly repeat
        d.repeat = RepeatMode::Every(Freq::Weekly);
        d.days[6] = true;
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "pick:skip-date"), |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "pick-next"), |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "pick-day:2026-12-27"), |ui| show_event(ui, &mut d, &mut store));
        assert_eq!(d.skipped.iter().map(|r| date_text(r.day)).collect::<Vec<_>>(), vec!["2026-12-27"], "{:?}", d.error);
        // last day of the repeat
        d.end = EndMode::Until;
        d.until = "2027-02-01".into();
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "pick:until"), |ui| show_event(ui, &mut d, &mut store));
        assert!(d.rects.iter().any(|r| r.0 == "pick-day:2027-02-10"), "opens on the month of the field");
        h.click(rect(&d.rects, "pick-day:2027-02-10"), |ui| show_event(ui, &mut d, &mut store));
        assert_eq!(d.until, "2027-02-10");
        // weeks start on Monday or on Sunday
        assert_eq!(t::weekday(grid_start(2026, 11, true)), 0);
        assert_eq!(t::weekday(grid_start(2026, 11, false)), 6);
        assert_eq!(t::civil_from_days(grid_start(2026, 11, true)), (2026, 10, 26), "1 November 2026 is a Sunday: a Monday grid starts a week earlier");
        d.week_monday = false;
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "pick:start-date"), |ui| show_event(ui, &mut d, &mut store));
        let first = rect(&d.rects, "pick-day:2026-11-29").min.x;
        assert!(d.rects.iter().filter(|r| r.0.starts_with("pick-day:")).all(|r| r.1.min.x >= first - 0.5), "Sunday is the first column");
    }
}
