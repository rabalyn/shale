//! Calendar settings dialog (local calendars, CalDAV accounts and their sync intervals) and
//! the event dialog (new events and changes, saved to any configured calendar).

use crate::cal_dav::{self, Account, FoundCalendar};
use crate::cal_ics::Event;
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
    pub dav_pass: String,
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
        CalendarDialog { sources, week_monday, new_local_name: String::new(), dav_url: String::new(), dav_user: String::new(), dav_pass: String::new(), found: vec![], discovering: None, message: None, rects: vec![] }
    }

    fn start_discovery(&mut self) {
        if let Err(e) = cal_dav::check_url(self.dav_url.trim()) {
            self.message = Some(format!("Can't use this address: {e}"));
            return;
        }
        let acct = Account { url: self.dav_url.trim().to_string(), username: self.dav_user.clone(), password: self.dav_pass.clone() };
        let slot: Discovery = Arc::new(Mutex::new(None));
        let s2 = slot.clone();
        std::thread::spawn(move || {
            let r = cal_dav::discover(&acct);
            *s2.lock().unwrap() = Some(r);
        });
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
    ui.set_width(560.0);
    ui.label(egui::RichText::new("Calendars").size(18.0).color(theme::TEXT_STRONG));
    ui.add_space(4.0);
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
                    rec(format!("sync:{i}"), ui.add(egui::DragValue::new(&mut s.sync_minutes).range(0..=1440).suffix(" min")))
                        .on_hover_text("Load this calendar again every this many minutes (0 = only when Shale starts, or with Sync now)");
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
        rec("dav-pass".into(), ui.add(egui::TextEdit::singleline(&mut d.dav_pass).password(true).desired_width(220.0)));
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
                    ui.label(egui::RichText::new(format!("on {}", cal_dav::host_of(&c.url))).size(12.0).color(theme::LINK_MISSING)).on_hover_text("This calendar is on another server than the address you entered; your user name and password will be sent there.");
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

/// A unique ID for a new event.
fn new_uid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("shale-{nanos:x}-{:x}-{}@shale", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

impl EventDialog {
    fn blank(calendar: String) -> Self {
        EventDialog { calendar, original: None, title: String::new(), start_date: String::new(), start_time: String::new(), end_date: String::new(), end_time: String::new(), all_day: false, location: String::new(), description: String::new(), error: None, job: None, rects: vec![], focused: false, new_uid: new_uid() }
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
        d.original = Some((source_id, e));
        d
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
        Ok(e)
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

/// Show the event dialog and run its save. `store` is changed only after a finished save.
pub fn show_event(ui: &mut Ui, d: &mut EventDialog, store: &mut CalendarStore) -> Option<EventResult> {
    if let Some(msg) = d.finish(store) {
        return Some(EventResult::Saved(msg));
    }
    let mut rects: Vec<(String, Rect)> = Vec::new();
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
    ui.set_width(460.0);
    let heading = if d.original.is_some() { "Edit event" } else { "New event" };
    ui.label(egui::RichText::new(heading).size(17.0).color(theme::TEXT_STRONG));
    ui.add_space(2.0);
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
            let current = store.sources.iter().find(|s| s.id == d.calendar);
            let current_name = current.map(|s| s.name.clone()).unwrap_or_else(|| "(choose a calendar)".into());
            let combo = egui::ComboBox::from_id_salt("event-cal").width(320.0).selected_text(current_name).show_ui(ui, |ui| {
                for s in &store.sources {
                    let read_only = store.read_only.contains(&s.id);
                    let kind = if s.is_local() { "local" } else if read_only { "CalDAV, read-only" } else { "CalDAV" };
                    let label = egui::RichText::new(format!("● {}   ({kind})", s.name)).color(crate::drawing::parse_color(&s.color).unwrap_or(theme::TEXT));
                    if rec(&format!("cal:{}", s.name), ui.add_enabled(!read_only || d.calendar == s.id, egui::Button::selectable(d.calendar == s.id, label))).clicked() {
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
                start_changed = a.changed() || b_changed;
                rec("all-day", ui.checkbox(&mut d.all_day, "All day"));
            });
            ui.end_row();
            ui.label(muted("Ends"));
            ui.horizontal(|ui| {
                rec("end-date", ui.add(egui::TextEdit::singleline(&mut d.end_date).desired_width(96.0)));
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
            ui.label(muted("This event repeats: changes apply to every repeat."));
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
        h.click(rect(&d.rects, "save"), |ui| show_event(ui, &mut d, &mut store));
        let r = wait_saved(&mut h, &mut d, &mut store);
        assert!(matches!(&r, Some(EventResult::Saved(m)) if m.starts_with("Saved")), "{r:?} {:?}", d.error);
        let saved = store.find(&src.id, &e.uid).expect("the event is in the calendar");
        assert_eq!(saved.summary, "Dentist");
        // editing keeps the event's ID and changes it in place
        let mut d = EventDialog::edit(src.id.clone(), saved.clone());
        assert_eq!((d.title.as_str(), d.start_time.as_str(), d.end_time.as_str()), ("Dentist", "14:00", "15:00"));
        d.title = "Dentist (moved)".into();
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "save"), |ui| show_event(ui, &mut d, &mut store));
        wait_saved(&mut h, &mut d, &mut store).unwrap();
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
        let dav = |id: &str, name: &str| CalendarSource { id: id.into(), name: name.into(), color: "#1971c2".into(), enabled: true, sync_minutes: 0, kind: SourceKind::Caldav { url: format!("{base}/{id}/"), username: "ada".into(), password: "x".into() } };
        store.set_sources(vec![local.clone(), dav("shared", "Shared"), dav("work", "Work")]);
        let mut d = EventDialog::new_at("shared".into(), 1_800_000_000);
        d.title = "Kickoff".into();
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        h.frame(vec![], |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "save"), |ui| show_event(ui, &mut d, &mut store));
        assert!(wait_saved(&mut h, &mut d, &mut store).is_none(), "the dialog stays open");
        assert!(d.error.as_deref().unwrap_or("").contains("permission"), "{:?}", d.error);
        // pick another calendar and save again
        h.click(rect(&d.rects, "calendar"), |ui| show_event(ui, &mut d, &mut store));
        h.click(rect(&d.rects, "cal:Work"), |ui| show_event(ui, &mut d, &mut store));
        assert_eq!(d.calendar, "work");
        assert!(d.error.is_none());
        h.click(rect(&d.rects, "save"), |ui| show_event(ui, &mut d, &mut store));
        let r = wait_saved(&mut h, &mut d, &mut store);
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
        h.click(rect(&d.rects, "save"), |ui| show_event(ui, &mut d, &mut store));
        let r = wait_saved(&mut h, &mut d, &mut store);
        assert!(matches!(&r, Some(EventResult::Saved(m)) if m.starts_with("Moved")), "{r:?} {:?}", d.error);
        assert!(store.find(&local.id, "loc1").is_none(), "removed from the old calendar after the new one accepted it");
    }
}
