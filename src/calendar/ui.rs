//! The calendar panel: month, week and day views in three sizes, and an event details popup.

use crate::calendar::store::{CalendarStore, Occ};
use crate::calendar::time::{self as t, civil_from_days, days_from_civil, MONTHS, WEEKDAYS};
use crate::theme;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Month,
    Week,
    Day,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Size {
    Small,
    Medium,
    Large,
}

pub struct Popup {
    pub occ: Occ,
    pub pos: Pos2,
    opened_frame: u64,
    /// a delete button was clicked and the popup asks "Delete?" (CAL-011): None, or what goes
    confirm: Option<Confirm>,
}

/// What a delete confirmation is about.
#[derive(Clone, Copy, PartialEq)]
enum Confirm {
    /// the event (all its repeats)
    All,
    /// one date of a repeating event
    One,
}

pub enum CalAction {
    Settings,
    OpenLarge,
    /// open the event dialog for a new event starting at this time (UTC seconds)
    NewEventAt(i64),
    /// open the event dialog for this event
    Edit(Occ),
    /// remove this event (a repeating one with all its repeats) from its calendar
    Delete(Occ),
    /// open the event dialog for this one date of a repeating event (CAL-007)
    EditOne(Occ),
    /// remove this one date of a repeating event (CAL-007)
    DeleteOne(Occ),
    /// open the event's link (checked before it is opened)
    OpenUrl(String),
}

/// Round a time up to the next full or half hour (local clock); a time already on one stays.
pub fn next_half_hour(utc: i64) -> i64 {
    let (day, secs) = t::utc_to_local(utc);
    let up = (secs + 1799) / 1800 * 1800;
    t::local_to_utc(day + up / 86_400, up % 86_400)
}

/// A new event on `day` starts at the next full or half hour of the current time of day.
pub fn new_event_start(day: i64) -> i64 {
    let (_, secs) = t::utc_to_local(t::now());
    next_half_hour(t::local_to_utc(day, secs))
}

/// An event being dragged out of the calendar (to import it as a note).
#[derive(Clone, Debug)]
pub struct CalDrag {
    /// the event, with the dragged occurrence's start and end
    pub event: crate::calendar::ics::Event,
    pub calendar: String,
}

impl CalDrag {
    fn new(store: &CalendarStore, o: &Occ) -> Option<Self> {
        let mut event = store.event(o)?.clone();
        if event.rrule.is_some() && event.recurrence_id.is_none() {
            // one date of a repeating event
            event.recurrence_id = Some(o.start);
        }
        event.start = o.start;
        event.end = o.end;
        event.href = None;
        event.etag = None;
        Some(CalDrag { event, calendar: store.sources.get(o.source).map(|s| s.name.clone()).unwrap_or_default() })
    }
}

/// The calendar state of the window: the small view of the sidebar, the main view, the deletes
/// started from the event popup, and the calendar new events go to by default.
pub struct CalendarUi {
    pub side: CalendarView,
    pub main: CalendarView,
    /// deletes started from the event popup: (job, calendar)
    pub jobs: Vec<(crate::calendar::store::Job, String)>,
    /// the calendar the last event was saved to (the default for the next new one)
    pub last_event_calendar: Option<String>,
}

impl CalendarUi {
    pub fn new() -> Self {
        Self { side: CalendarView::new(Size::Small), main: CalendarView::new(Size::Medium), jobs: vec![], last_event_calendar: None }
    }
}

pub struct CalendarView {
    pub mode: Mode,
    pub size: Size,
    /// the day the view is showing (local day number)
    pub anchor: i64,
    /// small month view: the day whose events are listed under the grid
    pub selected: i64,
    pub popup: Option<Popup>,
    /// screen rect of every control / event drawn (used by the UI tests)
    pub rects: Vec<(String, Rect)>,
    scroll_done: bool,
    popup_rect: Option<Rect>,
    frame: u64,
}

fn parse_hex(c: &str) -> Color32 {
    crate::drawing::parse_color(c).unwrap_or(theme::ACCENT)
}

/// "15 minutes before" for a reminder.
pub fn reminder_text(a: &crate::calendar::ics::Alarm) -> String {
    let say = |n: i64, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
    match a.before {
        Some(0) => "at the start".into(),
        Some(b) if b > 0 && b % 86_400 == 0 => format!("{} before", say(b / 86_400, "day")),
        Some(b) if b > 0 && b % 3600 == 0 => format!("{} before", say(b / 3600, "hour")),
        Some(b) if b > 0 => format!("{} before", say((b + 59) / 60, "minute")),
        Some(b) => format!("{} after the start", say((-b + 59) / 60, "minute")),
        None => "set in another program".into(),
    }
}

pub fn rrule_text(rule: &str) -> String {
    let get = |k: &str| rule.split(';').find_map(|p| p.split_once('=').filter(|(a, _)| a.eq_ignore_ascii_case(k)).map(|(_, b)| b.to_string()));
    let n: i64 = get("INTERVAL").and_then(|v| v.parse().ok()).unwrap_or(1);
    let every = |one: &str, many: &str| if n == 1 { one.to_string() } else { format!("every {n} {many}") };
    let mut s = match get("FREQ").unwrap_or_default().to_ascii_uppercase().as_str() {
        "DAILY" => every("daily", "days"),
        "WEEKLY" => every("weekly", "weeks"),
        "MONTHLY" => every("monthly", "months"),
        "YEARLY" => every("yearly", "years"),
        _ => "repeating".into(),
    };
    if let Some(days) = get("BYDAY") {
        let names: Vec<&str> = days
            .split(',')
            .filter_map(|d| match d.trim_start_matches(|c: char| c == '-' || c == '+' || c.is_ascii_digit()) {
                "MO" => Some("Mon"),
                "TU" => Some("Tue"),
                "WE" => Some("Wed"),
                "TH" => Some("Thu"),
                "FR" => Some("Fri"),
                "SA" => Some("Sat"),
                "SU" => Some("Sun"),
                _ => None,
            })
            .collect();
        s.push_str(&format!(" on {}", names.join(", ")));
    }
    if let Some(c) = get("COUNT") {
        s.push_str(&format!(", {c} times"));
    }
    s
}

/// "Thu, 12 Nov 2026 · 14:00 – 15:30", "Fri, 20 Nov – Sun, 22 Nov 2026 (3 days)", …
pub fn when_text(o: &Occ) -> String {
    if o.all_day {
        let (a, b) = (o.start_day(), o.end_day());
        if a == b {
            format!("{} · all day", t::long_date(a))
        } else {
            format!("{} – {} ({} days)", t::long_date(a), t::long_date(b), b - a + 1)
        }
    } else {
        let (sd, ss) = t::utc_to_local(o.start);
        let (ed, es) = t::utc_to_local(o.end);
        if sd == ed || (ed == sd + 1 && es == 0) {
            format!("{} · {} – {}", t::long_date(sd), t::hhmm(ss), if es == 0 && ed > sd { "24:00".into() } else { t::hhmm(es) })
        } else {
            format!("{} {} – {} {}", t::long_date(sd), t::hhmm(ss), t::long_date(ed), t::hhmm(es))
        }
    }
}

/// One event chip to draw: where, which occurrence, the key that tells it apart, and whether it shows its time.
struct ChipSpec<'a> {
    rect: Rect,
    occ: &'a Occ,
    key: String,
    show_time: bool,
}

impl CalendarView {
    pub fn new(size: Size) -> Self {
        let today = t::today();
        CalendarView { mode: Mode::Month, size, anchor: today, selected: today, popup: None, rects: vec![], scroll_done: false, popup_rect: None, frame: 0 }
    }

    fn rec(&mut self, key: impl Into<String>, r: Rect) {
        self.rects.push((key.into(), r));
    }

    fn week_start(&self, day: i64, monday: bool) -> i64 {
        let off = if monday { t::weekday(day) as i64 } else { (t::weekday(day) as i64 + 1) % 7 };
        day - off
    }

    /// Draw the whole panel. `large`: shown as a big tab (more room) instead of the sidebar.
    pub fn show(&mut self, ui: &mut Ui, store: &CalendarStore, monday: bool, large: bool) -> Option<CalAction> {
        self.frame += 1;
        self.rects.clear();
        let mut action = None;
        // ---- header
        let title = match self.mode {
            Mode::Month => {
                let (y, m, _) = civil_from_days(self.anchor);
                format!("{} {y}", MONTHS[m as usize - 1])
            }
            Mode::Week => {
                let a = self.week_start(self.anchor, monday);
                let (y1, m1, d1) = civil_from_days(a);
                let (y2, m2, d2) = civil_from_days(a + 6);
                if m1 == m2 {
                    format!("{d1} – {d2} {} {y2}", &MONTHS[m2 as usize - 1][..3])
                } else if y1 == y2 {
                    format!("{d1} {} – {d2} {} {y2}", &MONTHS[m1 as usize - 1][..3], &MONTHS[m2 as usize - 1][..3])
                } else {
                    format!("{d1} {} {y1} – {d2} {} {y2}", &MONTHS[m1 as usize - 1][..3], &MONTHS[m2 as usize - 1][..3])
                }
            }
            Mode::Day => t::long_date(self.anchor),
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let b = ui.button("‹").on_hover_text("Previous");
            self.rec("prev", b.rect);
            if b.clicked() {
                self.step(-1);
            }
            let b = ui.button("Today");
            self.rec("today", b.rect);
            if b.clicked() {
                self.anchor = t::today();
                self.selected = self.anchor;
            }
            let b = ui.button("›").on_hover_text("Next");
            self.rec("next", b.rect);
            if b.clicked() {
                self.step(1);
            }
            ui.label(egui::RichText::new(&title).size(if large { 18.0 } else { 14.0 }).color(theme::TEXT_STRONG));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let b = ui.small_button("⚙").on_hover_text("Calendar settings");
                self.rec("settings", b.rect);
                if b.clicked() {
                    action = Some(CalAction::Settings);
                }
                if !large {
                    let b = ui.small_button("⤢").on_hover_text("Open the calendar in a big tab");
                    self.rec("large", b.rect);
                    if b.clicked() {
                        action = Some(CalAction::OpenLarge);
                    }
                }
                if !store.sources.is_empty() {
                    let b = ui.small_button("＋").on_hover_text("New event (or double-click a day or time)");
                    self.rec("new", b.rect);
                    if b.clicked() {
                        let day = if self.mode == Mode::Month { self.selected } else { self.anchor };
                        action = Some(CalAction::NewEventAt(new_event_start(day)));
                    }
                }
                if store.is_loading() {
                    ui.spinner();
                }
            });
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (m, label) in [(Mode::Month, "Month"), (Mode::Week, "Week"), (Mode::Day, "Day")] {
                let b = ui.selectable_label(self.mode == m, label);
                self.rec(format!("mode:{label}"), b.rect);
                if b.clicked() {
                    self.mode = m;
                    self.scroll_done = false;
                }
            }
            ui.separator();
            for (s, label, tip) in [(Size::Small, "S", "Small"), (Size::Medium, "M", "Medium: room for a few events per day"), (Size::Large, "L", "Large: big days for busy schedules")] {
                let b = ui.selectable_label(self.size == s, label).on_hover_text(tip);
                self.rec(format!("size:{label}"), b.rect);
                if b.clicked() {
                    self.size = s;
                    self.scroll_done = false;
                }
            }
        });
        if store.sources.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("No calendars yet.").color(theme::MUTED));
            let b = ui.button("Add a calendar…");
            self.rec("add-calendar", b.rect);
            if b.clicked() {
                action = Some(CalAction::Settings);
            }
        }
        ui.add_space(4.0);
        let mut clicked: Option<(Occ, Pos2)> = None;
        let mut create: Option<i64> = None;
        match self.mode {
            Mode::Month => self.month(ui, store, monday, large, &mut clicked, &mut create),
            Mode::Week => {
                let a = self.week_start(self.anchor, monday);
                self.time_grid(ui, store, (a..a + 7).collect(), large, &mut clicked, &mut create);
            }
            Mode::Day => {
                let d = self.anchor;
                self.time_grid(ui, store, vec![d], large, &mut clicked, &mut create);
            }
        }
        if let Some(start) = create {
            if store.sources.is_empty() {
                action = Some(CalAction::Settings);
            } else {
                action = Some(CalAction::NewEventAt(start));
            }
        }
        if let Some((occ, pos)) = clicked {
            self.popup = Some(Popup { occ, pos, opened_frame: self.frame, confirm: None });
        }
        if let Some(a) = self.popup_ui(ui, store) {
            action = Some(a);
        }
        action
    }

    fn step(&mut self, dir: i64) {
        match self.mode {
            Mode::Month => {
                let (y, m, _) = civil_from_days(self.anchor);
                let (y2, m2) = t::add_months(y, m, dir as i32);
                self.anchor = days_from_civil(y2, m2, 1);
                self.selected = self.anchor;
            }
            Mode::Week => self.anchor += 7 * dir,
            Mode::Day => self.anchor += dir,
        }
    }

    fn chip(&mut self, ui: &mut Ui, chip: ChipSpec, store: &CalendarStore, clicked: &mut Option<(Occ, Pos2)>) {
        let ChipSpec { rect, occ: o, key, show_time } = chip;
        let Some(e) = store.event(o) else { return };
        let col = parse_hex(&store.sources[o.source].color);
        let p = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        if o.all_day {
            p.rect_filled(rect, CornerRadius::same(3), col.gamma_multiply(0.85));
        } else {
            p.rect_filled(rect, CornerRadius::same(3), col.gamma_multiply(0.22));
            p.rect_filled(Rect::from_min_size(rect.min, Vec2::new(3.0, rect.height())), CornerRadius::same(1), col);
        }
        let label = if show_time && !o.all_day { format!("{} {}", t::hhmm(t::utc_to_local(o.start).1), e.summary) } else { e.summary.clone() };
        let size = (rect.height() * 0.68).clamp(9.0, 13.0);
        p.text(rect.left_center() + Vec2::new(if o.all_day { 4.0 } else { 6.0 }, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(size), if o.all_day { Color32::WHITE } else { theme::TEXT });
        let resp = ui.interact(rect, ui.scope_id().with((&key, o.start)), Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::PointingHand);
        self.rec(key, rect);
        if resp.clicked() {
            *clicked = Some((o.clone(), resp.interact_pointer_pos().unwrap_or(rect.center())));
        }
        if resp.drag_started() {
            if let Some(d) = CalDrag::new(store, o) {
                egui::DragAndDrop::set_payload(ui.ctx(), d);
            }
        }
    }

    fn month(&mut self, ui: &mut Ui, store: &CalendarStore, monday: bool, large: bool, clicked: &mut Option<(Occ, Pos2)>, create: &mut Option<i64>) {
        let (y, m, _) = civil_from_days(self.anchor);
        let first = days_from_civil(y, m, 1);
        let start = self.week_start(first, monday);
        let rows = ((first - start + t::days_in_month(y, m) as i64) + 6) / 7;
        let occs = store.occurrences(start, start + rows * 7 - 1);
        let mut by_day: Vec<Vec<&Occ>> = vec![vec![]; (rows * 7) as usize];
        for o in &occs {
            for d in o.start_day().max(start)..=o.end_day().min(start + rows * 7 - 1) {
                by_day[(d - start) as usize].push(o);
            }
        }
        let w = ui.available_width();
        let cw = w / 7.0;
        let avail_h = ui.available_height();
        let ch = match self.size {
            Size::Small => {
                if large {
                    44.0
                } else {
                    30.0
                }
            }
            Size::Medium => 74.0,
            Size::Large => {
                if large {
                    ((avail_h - 24.0) / rows as f32).max(130.0)
                } else {
                    120.0
                }
            }
        };
        // weekday header
        let (hr, _) = ui.allocate_exact_size(Vec2::new(w, 18.0), Sense::hover());
        for i in 0..7 {
            let wd = if monday { i } else { (i + 6) % 7 };
            let name = if large { WEEKDAYS[wd] } else { &WEEKDAYS[wd][..2] };
            ui.painter().text(Pos2::new(hr.min.x + cw * (i as f32 + 0.5), hr.center().y), Align2::CENTER_CENTER, name, FontId::proportional(11.0), theme::MUTED);
        }
        let today = t::today();
        let (grid, _) = ui.allocate_exact_size(Vec2::new(w, ch * rows as f32), Sense::hover());
        for i in 0..rows * 7 {
            let d = start + i;
            let r = Rect::from_min_size(grid.min + Vec2::new(cw * (i % 7) as f32, ch * (i / 7) as f32), Vec2::new(cw, ch)).shrink(1.0);
            let in_month = civil_from_days(d).1 == m;
            let resp = ui.interact(r, ui.scope_id().with(("cal-day", d)), Sense::click());
            self.rec(format!("day:{d}"), r);
            let p = ui.painter();
            let bg = if self.size == Size::Small {
                Color32::TRANSPARENT
            } else if in_month {
                theme::BG_SECONDARY
            } else {
                theme::BG_PRIMARY
            };
            p.rect_filled(r, CornerRadius::same(4), if resp.hovered() { theme::BG_HOVER } else { bg });
            if self.size == Size::Small && d == self.selected {
                p.rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, theme::ACCENT), egui::StrokeKind::Inside);
            }
            let num_pos = if self.size == Size::Small { Pos2::new(r.center().x, r.min.y + 10.0) } else { Pos2::new(r.min.x + 12.0, r.min.y + 9.0) };
            if d == today {
                p.circle_filled(num_pos, 9.0, theme::ACCENT);
            }
            let (_, _, dd) = civil_from_days(d);
            let numc = if d == today {
                Color32::WHITE
            } else if in_month {
                theme::TEXT
            } else {
                theme::FAINT
            };
            p.text(num_pos, Align2::CENTER_CENTER, dd.to_string(), FontId::proportional(11.5), numc);
            let list = &by_day[i as usize];
            match self.size {
                Size::Small => {
                    let n = list.len().min(4);
                    let total = n as f32 * 6.0;
                    for (k, o) in list.iter().take(4).enumerate() {
                        let c = parse_hex(&store.sources[o.source].color);
                        p.circle_filled(Pos2::new(r.center().x - total / 2.0 + 3.0 + k as f32 * 6.0, r.max.y - 5.0), 2.2, c);
                    }
                    if resp.clicked() {
                        self.selected = d;
                    }
                    if resp.double_clicked() {
                        self.selected = d;
                        *create = Some(new_event_start(d));
                    }
                }
                _ => {
                    let chip_h = if self.size == Size::Medium { 15.0 } else { 18.0 };
                    let fit = (((r.height() - 20.0) / (chip_h + 2.0)).floor() as usize).max(1);
                    let shown = if list.len() > fit { fit - 1 } else { list.len() };
                    for (k, o) in list.iter().take(shown).enumerate() {
                        let cr = Rect::from_min_size(Pos2::new(r.min.x + 2.0, r.min.y + 19.0 + k as f32 * (chip_h + 2.0)), Vec2::new(r.width() - 4.0, chip_h));
                        let summary = store.event(o).map(|e| e.summary.clone()).unwrap_or_default();
                        let o2 = (*o).clone();
                        self.chip(ui, ChipSpec { rect: cr, occ: &o2, key: format!("event:{summary}:{d}"), show_time: true }, store, clicked);
                    }
                    if list.len() > shown {
                        let mr = Rect::from_min_size(Pos2::new(r.min.x + 2.0, r.min.y + 19.0 + shown as f32 * (chip_h + 2.0)), Vec2::new(r.width() - 4.0, chip_h));
                        let more = ui.interact(mr, ui.scope_id().with(("more", d)), Sense::click()).on_hover_text("Show this day");
                        ui.painter().text(mr.left_center() + Vec2::new(4.0, 0.0), Align2::LEFT_CENTER, format!("+{} more", list.len() - shown), FontId::proportional(11.0), theme::MUTED);
                        self.rec(format!("more:{d}"), mr);
                        if more.clicked() {
                            self.mode = Mode::Day;
                            self.anchor = d;
                            self.scroll_done = false;
                        }
                    }
                    if resp.double_clicked() {
                        *create = Some(new_event_start(d));
                    }
                }
            }
        }
        if self.size == Size::Small {
            // the selected day's events under the small grid
            ui.add_space(6.0);
            ui.label(egui::RichText::new(t::long_date(self.selected)).size(12.5).color(theme::TEXT_STRONG));
            let day_occ: Vec<Occ> = store.occurrences(self.selected, self.selected);
            if day_occ.is_empty() {
                ui.label(egui::RichText::new("No events").size(12.0).color(theme::MUTED));
            }
            for o in day_occ {
                let Some(e) = store.event(&o) else { continue };
                let summary = e.summary.clone();
                let time = if o.all_day { "all day".to_string() } else { format!("{}–{}", t::hhmm(t::utc_to_local(o.start).1), t::hhmm(t::utc_to_local(o.end).1)) };
                let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
                let label_o = Occ { all_day: o.all_day, ..o.clone() };
                self.chip(ui, ChipSpec { rect: r, occ: &label_o, key: format!("agenda:{summary}"), show_time: false }, store, clicked);
                ui.painter().text(r.right_center() - Vec2::new(6.0, 0.0), Align2::RIGHT_CENTER, time, FontId::proportional(11.0), if o.all_day { Color32::WHITE } else { theme::MUTED });
            }
        }
    }

    fn time_grid(&mut self, ui: &mut Ui, store: &CalendarStore, days: Vec<i64>, large: bool, clicked: &mut Option<(Occ, Pos2)>, create: &mut Option<i64>) {
        let n = days.len();
        let w = ui.available_width();
        let gutter = 40.0;
        let col_w = (w - gutter) / n as f32;
        let today = t::today();
        let occs = store.occurrences(days[0], days[n - 1]);
        // day headers
        let (hr, _) = ui.allocate_exact_size(Vec2::new(w, 22.0), Sense::hover());
        for (i, d) in days.iter().enumerate() {
            let r = Rect::from_min_size(Pos2::new(hr.min.x + gutter + col_w * i as f32, hr.min.y), Vec2::new(col_w, 22.0));
            let (_, _, dd) = civil_from_days(*d);
            let label = format!("{} {dd}", WEEKDAYS[t::weekday(*d) as usize]);
            let resp = ui.interact(r, ui.scope_id().with(("cal-head", *d)), Sense::click()).on_hover_text("Show this day");
            self.rec(format!("day:{d}"), r);
            if *d == today {
                ui.painter().rect_filled(r.shrink(2.0), CornerRadius::same(4), theme::ACCENT.gamma_multiply(0.35));
            }
            ui.painter().text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(12.0), if *d == today { theme::TEXT_STRONG } else { theme::MUTED });
            if resp.clicked() && n > 1 {
                self.mode = Mode::Day;
                self.anchor = *d;
                self.scroll_done = false;
            }
        }
        // all-day lane
        let lane: Vec<Vec<Occ>> = days.iter().map(|d| occs.iter().filter(|o| o.all_day && o.start_day() <= *d && o.end_day() >= *d).cloned().collect()).collect();
        let rows = lane.iter().map(|l| l.len()).max().unwrap_or(0).min(4);
        if rows > 0 {
            let (lr, _) = ui.allocate_exact_size(Vec2::new(w, rows as f32 * 18.0 + 4.0), Sense::hover());
            ui.painter().text(Pos2::new(lr.min.x + 2.0, lr.min.y + 9.0), Align2::LEFT_CENTER, "all day", FontId::proportional(10.0), theme::FAINT);
            for (i, list) in lane.iter().enumerate() {
                for (k, o) in list.iter().take(rows).enumerate() {
                    let r = Rect::from_min_size(Pos2::new(lr.min.x + gutter + col_w * i as f32 + 1.0, lr.min.y + 2.0 + k as f32 * 18.0), Vec2::new(col_w - 2.0, 16.0));
                    let summary = store.event(o).map(|e| e.summary.clone()).unwrap_or_default();
                    self.chip(ui, ChipSpec { rect: r, occ: o, key: format!("event:{summary}:{}", days[i]), show_time: false }, store, clicked);
                }
            }
        }
        // hours
        let hour_h = match self.size {
            Size::Small => 20.0,
            Size::Medium => 36.0,
            Size::Large => 62.0,
        };
        let height = if large { ui.available_height().max(200.0) } else { 380.0 };
        let mut sa = egui::ScrollArea::vertical().id_salt(("cal-hours", large)).max_height(height).auto_shrink([false, false]);
        if !self.scroll_done {
            sa = sa.vertical_scroll_offset(7.0 * hour_h);
            self.scroll_done = true;
        }
        sa.show(ui, |ui| {
            let (gr, grid_resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0 * hour_h), Sense::click());
            let col_w = (gr.width() - gutter) / n as f32;
            self.rec("hours", gr);
            // double-click an empty spot: a new event from the next half hour there
            if grid_resp.double_clicked() {
                if let Some(p) = grid_resp.interact_pointer_pos() {
                    let col = ((p.x - gr.min.x - gutter) / col_w).floor();
                    if col >= 0.0 && (col as usize) < n {
                        let secs = (((p.y - gr.min.y) / hour_h * 3600.0) as i64).clamp(0, 86_399);
                        *create = Some(next_half_hour(t::local_to_utc(days[col as usize], secs)));
                    }
                }
            }
            let p = ui.painter();
            for h in 0..24 {
                let y = gr.min.y + h as f32 * hour_h;
                p.line_segment([Pos2::new(gr.min.x + gutter, y), Pos2::new(gr.max.x, y)], Stroke::new(1.0, theme::BORDER));
                if h > 0 {
                    p.text(Pos2::new(gr.min.x + gutter - 4.0, y), Align2::RIGHT_CENTER, format!("{h:02}:00"), FontId::proportional(10.0), theme::FAINT);
                }
            }
            for i in 0..=n {
                let x = gr.min.x + gutter + col_w * i as f32;
                p.line_segment([Pos2::new(x, gr.min.y), Pos2::new(x, gr.max.y)], Stroke::new(1.0, theme::BORDER));
            }
            for (i, d) in days.iter().enumerate() {
                // timed events of this day, clipped to the day; overlapping ones side by side
                let mut items: Vec<(Occ, f32, f32)> = occs
                    .iter()
                    .filter(|o| !o.all_day && o.start_day() <= *d && o.end_day() >= *d)
                    .map(|o| {
                        let day0 = t::local_to_utc(*d, 0);
                        let day1 = t::local_to_utc(*d + 1, 0);
                        let s = (o.start.max(day0) - day0) as f32 / 3600.0;
                        let e = (o.end.min(day1) - day0) as f32 / 3600.0;
                        (o.clone(), s, e.max(s + 0.25))
                    })
                    .collect();
                items.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
                let mut cols: Vec<f32> = Vec::new(); // end time per column
                let mut placed: Vec<(usize, usize)> = Vec::new(); // (column, cluster id)
                let mut cluster = 0;
                let mut cluster_end = f32::MIN;
                let mut cluster_cols: Vec<usize> = Vec::new();
                for it in &items {
                    if it.1 >= cluster_end {
                        cluster += 1;
                        cols.clear();
                        cluster_cols.push(0);
                    }
                    let c = match cols.iter().position(|end| *end <= it.1) {
                        Some(c) => {
                            cols[c] = it.2;
                            c
                        }
                        None => {
                            cols.push(it.2);
                            cols.len() - 1
                        }
                    };
                    cluster_end = cluster_end.max(it.2);
                    let last = cluster_cols.len() - 1;
                    cluster_cols[last] = cluster_cols[last].max(cols.len());
                    placed.push((c, cluster - 1));
                }
                for (it, (c, cl)) in items.iter().zip(placed) {
                    let ncols = cluster_cols[cl].max(1) as f32;
                    let x0 = gr.min.x + gutter + col_w * i as f32 + 1.0;
                    let r = Rect::from_min_max(Pos2::new(x0 + (col_w - 2.0) * c as f32 / ncols, gr.min.y + it.1 * hour_h + 1.0), Pos2::new(x0 + (col_w - 2.0) * (c as f32 + 1.0) / ncols - 1.0, gr.min.y + it.2 * hour_h - 1.0));
                    let summary = store.event(&it.0).map(|e| e.summary.clone()).unwrap_or_default();
                    self.block(ui, r, &it.0, store, format!("event:{summary}:{d}"), clicked);
                }
                if *d == today {
                    let (_, secs) = t::utc_to_local(t::now());
                    let y = gr.min.y + secs as f32 / 3600.0 * hour_h;
                    let x0 = gr.min.x + gutter + col_w * i as f32;
                    ui.painter().line_segment([Pos2::new(x0, y), Pos2::new(x0 + col_w, y)], Stroke::new(1.5, theme::LINK_MISSING));
                    ui.painter().circle_filled(Pos2::new(x0, y), 3.0, theme::LINK_MISSING);
                }
            }
        });
    }

    fn block(&mut self, ui: &mut Ui, r: Rect, o: &Occ, store: &CalendarStore, key: String, clicked: &mut Option<(Occ, Pos2)>) {
        let Some(e) = store.event(o) else { return };
        let col = parse_hex(&store.sources[o.source].color);
        let p = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
        p.rect_filled(r, CornerRadius::same(4), col.gamma_multiply(0.30));
        p.rect_filled(Rect::from_min_size(r.min, Vec2::new(3.0, r.height())), CornerRadius::same(1), col);
        let mut y = r.min.y + 2.0;
        let big = r.height() > 30.0;
        p.text(Pos2::new(r.min.x + 6.0, y), Align2::LEFT_TOP, &e.summary, FontId::proportional(11.5), theme::TEXT_STRONG);
        y += 14.0;
        if big {
            p.text(Pos2::new(r.min.x + 6.0, y), Align2::LEFT_TOP, format!("{} – {}", t::hhmm(t::utc_to_local(o.start).1), t::hhmm(t::utc_to_local(o.end).1)), FontId::proportional(10.0), theme::MUTED);
            y += 13.0;
            if !e.location.is_empty() && r.height() > 44.0 {
                p.text(Pos2::new(r.min.x + 6.0, y), Align2::LEFT_TOP, &e.location, FontId::proportional(10.0), theme::MUTED);
            }
        }
        let resp = ui.interact(r, ui.scope_id().with((&key, o.start)), Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.drag_started() {
            if let Some(d) = CalDrag::new(store, o) {
                egui::DragAndDrop::set_payload(ui.ctx(), d);
            }
        }
        self.rec(key, r);
        if resp.clicked() {
            *clicked = Some((o.clone(), resp.interact_pointer_pos().unwrap_or(r.center())));
        }
    }

    fn popup_ui(&mut self, ui: &mut Ui, store: &CalendarStore) -> Option<CalAction> {
        let Some(popup) = &self.popup else {
            self.popup_rect = None;
            return None;
        };
        let Some(e) = store.event(&popup.occ).cloned() else {
            self.popup = None;
            return None;
        };
        let src = store.sources[popup.occ.source].clone();
        let col = parse_hex(&src.color);
        let screen = ui.ctx().content_rect();
        let w = 330.0;
        let pos = Pos2::new((popup.pos.x + 12.0).min(screen.max.x - w - 12.0).max(screen.min.x + 4.0), (popup.pos.y - 20.0).clamp(screen.min.y + 4.0, (screen.max.y - 380.0).max(screen.min.y + 4.0)));
        let occ = popup.occ.clone();
        let series = e.rrule.is_some() || e.recurrence_id.is_some();
        let opened = popup.opened_frame;
        let mut close = false;
        let mut action = None;
        let confirm = popup.confirm;
        let mut new_confirm = confirm;
        let mut rects: Vec<(String, Rect)> = Vec::new();
        let area = egui::Area::new(egui::Id::unique("cal-event-popup")).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                ui.set_width(w - 24.0);
                let (bar, _) = ui.allocate_exact_size(Vec2::new(w - 24.0, 4.0), Sense::hover());
                ui.painter().rect_filled(bar, CornerRadius::same(2), col);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(egui::RichText::new(if e.summary.is_empty() { "(no title)" } else { &e.summary }).size(17.0).color(theme::TEXT_STRONG)).wrap());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        let b = ui.small_button("✕").on_hover_text("Close (Esc)");
                        rects.push(("popup-x".into(), b.rect));
                        if b.clicked() {
                            close = true;
                        }
                    });
                });
                let row = |ui: &mut Ui, label: &str, value: &str| {
                    ui.horizontal_top(|ui| {
                        ui.add_sized([72.0, 16.0], egui::Label::new(egui::RichText::new(label).size(11.5).color(theme::MUTED)));
                        ui.add(egui::Label::new(egui::RichText::new(value).size(12.5).color(theme::TEXT)).wrap());
                    });
                };
                row(ui, "When", &when_text(&occ));
                if let Some(r) = &e.rrule {
                    row(ui, "Repeats", &rrule_text(r));
                }
                ui.horizontal(|ui| {
                    ui.add_sized([72.0, 16.0], egui::Label::new(egui::RichText::new("Calendar").size(11.5).color(theme::MUTED)));
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 4.5, col);
                    ui.label(egui::RichText::new(&src.name).size(12.5));
                });
                if !e.location.is_empty() {
                    row(ui, "Where", &e.location);
                }
                if !e.organizer.is_empty() {
                    row(ui, "Organizer", &e.organizer);
                }
                if !e.attendees.is_empty() {
                    row(ui, "Attendees", &e.attendees.join("\n"));
                }
                if !e.status.is_empty() {
                    row(ui, "Status", &e.status.to_lowercase());
                }
                if !e.categories.is_empty() {
                    row(ui, "Categories", &e.categories.join(", "));
                }
                if !e.alarms.is_empty() {
                    row(ui, "Reminders", &e.alarms.iter().map(reminder_text).collect::<Vec<_>>().join("\n"));
                }
                if !e.url.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_sized([72.0, 16.0], egui::Label::new(egui::RichText::new("Link").size(11.5).color(theme::MUTED)));
                        let l = ui.link(&e.url);
                        rects.push(("popup-url".into(), l.rect));
                        if l.clicked() {
                            action = Some(CalAction::OpenUrl(e.url.clone()));
                        }
                    });
                }
                if !e.description.is_empty() {
                    ui.separator();
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(&e.description).size(12.5).color(theme::TEXT)).wrap());
                    });
                }
                ui.add_space(6.0);
                if let Some(c) = confirm {
                    let name = if e.summary.is_empty() { "(no title)" } else { e.summary.as_str() };
                    let q = match c {
                        Confirm::All if series => format!("Delete “{name}” with all its repeats?"),
                        Confirm::All => format!("Delete “{name}”?"),
                        Confirm::One => format!("Delete only {} of “{name}”?", when_text(&occ)),
                    };
                    ui.add(egui::Label::new(egui::RichText::new(q).size(13.0).color(theme::TEXT_STRONG)).wrap());
                    ui.label(egui::RichText::new("A copy is kept, so it can be brought back by hand.").size(11.5).color(theme::MUTED));
                    ui.horizontal(|ui| {
                        let b = ui.button(egui::RichText::new("Delete").color(theme::LINK_MISSING));
                        rects.push(("popup-confirm".into(), b.rect));
                        if b.clicked() {
                            action = Some(if c == Confirm::One { CalAction::DeleteOne(occ.clone()) } else { CalAction::Delete(occ.clone()) });
                            close = true;
                        }
                        let b = ui.button("Keep");
                        rects.push(("popup-keep".into(), b.rect));
                        if b.clicked() {
                            new_confirm = None;
                        }
                    });
                    return;
                }
                ui.horizontal(|ui| {
                    let b = ui.button("Close");
                    rects.push(("popup-close".into(), b.rect));
                    if b.clicked() {
                        close = true;
                    }
                    if !series {
                        let b = ui.button("Edit").on_hover_text("Change the event or move it to another calendar");
                        rects.push(("popup-edit".into(), b.rect));
                        if b.clicked() {
                            action = Some(CalAction::Edit(occ.clone()));
                            close = true;
                        }
                        let b = ui.button(egui::RichText::new("Delete event").color(theme::LINK_MISSING)).on_hover_text(format!("Remove it from “{}”", src.name));
                        rects.push(("popup-delete".into(), b.rect));
                        if b.clicked() {
                            new_confirm = Some(Confirm::All);
                        }
                    }
                });
                // a repeating event: this date only, or all of them (CAL-007)
                if series {
                    ui.horizontal(|ui| {
                        let b = ui.button("Edit this date").on_hover_text("Change only this date; the other repeats stay as they are");
                        rects.push(("popup-edit-one".into(), b.rect));
                        if b.clicked() {
                            action = Some(CalAction::EditOne(occ.clone()));
                            close = true;
                        }
                        let b = ui.button("Edit all repeats").on_hover_text("Change the repeating event: its times, how it repeats, its reminders");
                        rects.push(("popup-edit".into(), b.rect));
                        if b.clicked() {
                            action = Some(CalAction::Edit(occ.clone()));
                            close = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        let b = ui.button(egui::RichText::new("Delete this date").color(theme::LINK_MISSING)).on_hover_text("Remove only this date; the other repeats stay");
                        rects.push(("popup-delete-one".into(), b.rect));
                        if b.clicked() {
                            new_confirm = Some(Confirm::One);
                        }
                        let b = ui.button(egui::RichText::new("Delete all repeats").color(theme::LINK_MISSING)).on_hover_text(format!("Remove it, with all its repeats, from “{}”", src.name));
                        rects.push(("popup-delete".into(), b.rect));
                        if b.clicked() {
                            new_confirm = Some(Confirm::All);
                        }
                    });
                }
            });
        });
        self.rects.extend(rects);
        let r = area.response.rect;
        self.popup_rect = Some(r);
        // Esc, or a click somewhere else, closes it (not the click that opened it)
        let (esc, pressed, ptr) = ui.input(|i| (i.key_pressed(egui::Key::Escape), i.pointer.any_pressed(), i.pointer.interact_pos()));
        if esc || (pressed && opened != self.frame && ptr.map(|p| !r.contains(p)).unwrap_or(false)) {
            close = true;
        }
        if close {
            self.popup = None;
        } else if let Some(p) = &mut self.popup {
            p.confirm = new_confirm;
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::store::{CalendarSource, SourceKind};
    use eframe::egui::{Event, Modifiers, PointerButton, RawInput};

    struct H {
        ctx: egui::Context,
        t: f64,
        actions: Vec<String>,
    }

    impl H {
        fn frame(&mut self, v: &mut CalendarView, store: &CalendarStore, large: bool, events: Vec<Event>) {
            self.t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 900.0))), time: Some(self.t), events, ..Default::default() };
            let mut act = None;
            let mut out = self.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    if let Some(a) = v.show(ui, store, true, large) {
                        act = Some(a);
                    }
                });
            });
            out.textures_delta.clear();
            if let Some(a) = act {
                self.actions.push(match a {
                    CalAction::Settings => "settings".into(),
                    CalAction::OpenUrl(u) => format!("url:{u}"),
                    CalAction::OpenLarge => "large".into(),
                    CalAction::NewEventAt(s) => format!("new:{s}"),
                    CalAction::Edit(o) => format!("edit:{}", store.event(&o).map(|e| e.summary.clone()).unwrap_or_default()),
                    CalAction::EditOne(o) => format!("edit-one:{}:{}", store.event(&o).map(|e| e.summary.clone()).unwrap_or_default(), o.start),
                    CalAction::DeleteOne(o) => format!("delete-one:{}:{}", store.event(&o).map(|e| e.uid.clone()).unwrap_or_default(), o.start),
                    CalAction::Delete(o) => format!("delete:{}", store.event(&o).map(|e| e.uid.clone()).unwrap_or_default()),
                });
            }
        }
        fn rect(v: &CalendarView, key: &str) -> Rect {
            v.rects.iter().find(|(k, _)| k == key || k.starts_with(&format!("{key}:"))).map(|r| r.1).unwrap_or_else(|| panic!("no {key}; have {:?}", v.rects.iter().map(|r| &r.0).take(80).collect::<Vec<_>>()))
        }
        fn click(&mut self, v: &mut CalendarView, store: &CalendarStore, large: bool, key: &str) {
            let p = Self::rect(v, key).center();
            self.click_at(v, store, large, p);
        }
        fn click_at(&mut self, v: &mut CalendarView, store: &CalendarStore, large: bool, p: Pos2) {
            let b = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            self.frame(v, store, large, vec![Event::PointerMoved(p)]);
            self.frame(v, store, large, vec![b(true)]);
            self.frame(v, store, large, vec![b(false)]);
            self.frame(v, store, large, vec![]);
        }
        fn dblclick_at(&mut self, v: &mut CalendarView, store: &CalendarStore, large: bool, p: Pos2) {
            let b = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            self.frame(v, store, large, vec![Event::PointerMoved(p)]);
            self.frame(v, store, large, vec![b(true)]);
            self.frame(v, store, large, vec![b(false)]);
            self.frame(v, store, large, vec![b(true)]);
            self.frame(v, store, large, vec![b(false)]);
            self.frame(v, store, large, vec![]);
        }
    }

    fn store_with_samples(tag: &str) -> (CalendarStore, String) {
        let dir = std::env::temp_dir().join(format!("shale-calui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = CalendarStore::new(vec![], dir);
        let src = s.new_local_source("Test", &[]);
        let id = src.id.clone();
        s.set_sources(vec![src]);
        s.fill_sample(&id).unwrap();
        (s, id)
    }

    #[test]
    fn view_buttons_switch_modes_sizes_and_dates() {
        let (store, _) = store_with_samples("modes");
        let mut v = CalendarView::new(Size::Small);
        let mut h = H { ctx: egui::Context::default(), t: 0.0, actions: vec![] };
        h.frame(&mut v, &store, false, vec![]);
        for (key, mode) in [("mode:Week", Mode::Week), ("mode:Day", Mode::Day), ("mode:Month", Mode::Month)] {
            h.click(&mut v, &store, false, key);
            assert_eq!(v.mode, mode, "{key}");
        }
        for (key, size) in [("size:M", Size::Medium), ("size:L", Size::Large), ("size:S", Size::Small)] {
            h.click(&mut v, &store, false, key);
            assert_eq!(v.size, size, "{key}");
        }
        let a = v.anchor;
        h.click(&mut v, &store, false, "next");
        assert!(v.anchor > a, "next month");
        h.click(&mut v, &store, false, "prev");
        h.click(&mut v, &store, false, "prev");
        assert!(v.anchor < a, "previous month");
        h.click(&mut v, &store, false, "today");
        assert_eq!(v.anchor, t::today());
        h.click(&mut v, &store, false, "settings");
        h.click(&mut v, &store, false, "large");
        let before = t::now();
        h.click(&mut v, &store, false, "new");
        assert_eq!(h.actions[..2], ["settings".to_string(), "large".into()]);
        let start: i64 = h.actions[2].strip_prefix("new:").unwrap().parse().unwrap();
        assert!(start >= before - 60 && start <= before + 1800, "the next half hour from now");
        assert_eq!(t::utc_to_local(start).1 % 1800, 0, "on :00 or :30");
        // week mode steps by 7 days, day mode by 1
        h.click(&mut v, &store, false, "mode:Week");
        let a = v.anchor;
        h.click(&mut v, &store, false, "next");
        assert_eq!(v.anchor, a + 7);
        h.click(&mut v, &store, false, "mode:Day");
        h.click(&mut v, &store, false, "next");
        assert_eq!(v.anchor, a + 8);
    }

    #[test]
    fn small_month_lists_the_selected_day_and_opens_details() {
        let (store, _) = store_with_samples("small");
        let mut v = CalendarView::new(Size::Small);
        let mut h = H { ctx: egui::Context::default(), t: 0.0, actions: vec![] };
        h.frame(&mut v, &store, false, vec![]);
        h.frame(&mut v, &store, false, vec![]);
        assert!(v.rects.iter().any(|r| r.0 == "agenda:Design review"), "today's events are listed under the grid");
        h.click(&mut v, &store, false, "agenda:Design review");
        let p = v.popup.as_ref().expect("clicking an event opens the details popup");
        assert_eq!(store.event(&p.occ).unwrap().summary, "Design review");
        assert!(v.rects.iter().any(|r| r.0 == "popup-close"));
        h.click(&mut v, &store, false, "popup-close");
        assert!(v.popup.is_none(), "Close button");
        // selecting the busy day lists its events
        let busy = t::today() + 2;
        h.click(&mut v, &store, false, &format!("day:{busy}"));
        assert_eq!(v.selected, busy);
        assert!(v.rects.iter().any(|r| r.0 == "agenda:Retro"));
        // Esc closes the popup, and so does a click elsewhere
        h.click(&mut v, &store, false, "agenda:Retro");
        assert!(v.popup.is_some());
        h.frame(&mut v, &store, false, vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        assert!(v.popup.is_none(), "Esc");
        h.click(&mut v, &store, false, "agenda:Retro");
        h.click_at(&mut v, &store, false, Pos2::new(1150.0, 880.0));
        assert!(v.popup.is_none(), "click outside");
    }

    #[test]
    fn bigger_month_shows_event_chips_and_more() {
        let (store, _) = store_with_samples("big");
        let mut v = CalendarView::new(Size::Medium);
        let mut h = H { ctx: egui::Context::default(), t: 0.0, actions: vec![] };
        h.frame(&mut v, &store, true, vec![]);
        let busy = t::today() + 2;
        let busy_month = civil_from_days(busy).1 == civil_from_days(t::today()).1;
        if busy_month {
            assert!(v.rects.iter().any(|r| r.0 == format!("more:{busy}")), "a busy day doesn't fit and shows '+n more'");
            h.click(&mut v, &store, true, &format!("more:{busy}"));
            assert_eq!(v.mode, Mode::Day);
            assert_eq!(v.anchor, busy);
            assert!(v.rects.iter().filter(|r| r.0.starts_with("event:") && r.0.ends_with(&format!(":{busy}"))).count() >= 10, "the day view shows all of them");
        }
        h.click(&mut v, &store, true, "mode:Month");
        h.click(&mut v, &store, true, "size:L");
        let today = t::today();
        let chips = v.rects.iter().filter(|r| r.0.starts_with("event:") && r.0.ends_with(&format!(":{today}"))).count();
        assert!(chips >= 4, "large month cells hold several events, got {chips}");
        h.click(&mut v, &store, true, &format!("event:Lunch with Sam:{today}"));
        assert_eq!(store.event(&v.popup.as_ref().unwrap().occ).unwrap().summary, "Lunch with Sam");
    }

    // spec: CAL-011
    #[test]
    fn week_view_blocks_and_delete_from_popup() {
        let (store, _) = store_with_samples("week");
        let mut v = CalendarView::new(Size::Medium);
        v.mode = Mode::Week;
        let mut h = H { ctx: egui::Context::default(), t: 0.0, actions: vec![] };
        h.frame(&mut v, &store, true, vec![]);
        h.frame(&mut v, &store, true, vec![]);
        let today = t::today();
        let review = H::rect(&v, &format!("event:Design review:{today}"));
        let lunch = H::rect(&v, &format!("event:Lunch with Sam:{today}"));
        assert!(review.height() > lunch.height(), "90 minutes are taller than 60");
        assert!(review.min.y < lunch.min.y, "10:00 is above 12:30");
        // overlapping events sit side by side
        let focus = H::rect(&v, &format!("event:Focus time:{today}"));
        let call = H::rect(&v, &format!("event:Call with supplier:{today}"));
        assert!(focus.max.x <= call.min.x + 1.0 || call.max.x <= focus.min.x + 1.0, "overlapping events don't cover each other");
        h.click(&mut v, &store, true, &format!("event:Gym:{today}"));
        assert!(v.popup.is_some());
        h.click(&mut v, &store, true, "popup-delete");
        assert!(h.actions.is_empty(), "the first click only asks: {:?}", h.actions);
        assert!(v.popup.is_some());
        // "Keep" goes back to the buttons without deleting
        h.click(&mut v, &store, true, "popup-keep");
        assert!(h.actions.is_empty() && v.popup.is_some());
        h.click(&mut v, &store, true, "popup-delete");
        h.click(&mut v, &store, true, "popup-confirm");
        assert!(h.actions.iter().any(|a| a.starts_with("delete:sample-gym")), "{:?}", h.actions);
        assert!(v.popup.is_none());
        h.click(&mut v, &store, true, &format!("event:Gym:{today}"));
        h.click(&mut v, &store, true, "popup-edit");
        assert_eq!(h.actions.last().unwrap(), "edit:Gym");
        assert!(v.popup.is_none());
        // clicking a day header shows that day
        h.click(&mut v, &store, true, &format!("day:{}", today));
        assert_eq!(v.mode, Mode::Day);
    }

    // spec: CAL-007
    #[test]
    fn a_repeating_event_offers_this_date_or_all_repeats() {
        let (store, _) = store_with_samples("series");
        let mut v = CalendarView::new(Size::Medium);
        v.mode = Mode::Week;
        let mut h = H { ctx: egui::Context::default(), t: 0.0, actions: vec![] };
        h.frame(&mut v, &store, true, vec![]);
        h.frame(&mut v, &store, true, vec![]);
        for (key, action) in [("popup-edit-one", "edit-one:Book club:"), ("popup-delete-one", "delete-one:sample-book"), ("popup-edit", "edit:Book club"), ("popup-delete", "delete:sample-book")] {
            h.click(&mut v, &store, true, "event:Book club");
            assert!(v.popup.is_some());
            h.click(&mut v, &store, true, key);
            if key.starts_with("popup-delete") {
                let n = h.actions.len();
                assert!(v.popup.is_some(), "{key}: the first click only asks");
                h.click(&mut v, &store, true, "popup-confirm");
                assert_eq!(h.actions.len(), n + 1, "{key}: only the confirmation sends the action");
            }
            assert!(h.actions.last().unwrap().starts_with(action), "{key}: {:?}", h.actions);
            assert!(v.popup.is_none());
        }
        // an event that does not repeat has the two plain buttons only
        h.click(&mut v, &store, true, "event:Gym");
        assert!(!v.rects.iter().any(|r| r.0 == "popup-edit-one" || r.0 == "popup-delete-one"));
        assert_eq!(reminder_text(&crate::calendar::ics::Alarm::before_start(900)), "15 minutes before");
        assert_eq!(reminder_text(&crate::calendar::ics::Alarm::before_start(86_400)), "1 day before");
        assert_eq!(reminder_text(&crate::calendar::ics::Alarm::before_start(0)), "at the start");
    }

    // spec: CAL-008
    #[test]
    fn next_half_hour_rounds_up() {
        let d = days_from_civil(2026, 11, 13);
        let at = |h: i64, m: i64| t::local_to_utc(d, h * 3600 + m * 60);
        assert_eq!(next_half_hour(at(10, 0)), at(10, 0));
        assert_eq!(next_half_hour(at(10, 1)), at(10, 30));
        assert_eq!(next_half_hour(at(10, 29)), at(10, 30));
        assert_eq!(next_half_hour(at(10, 31)), at(11, 0));
        assert_eq!(next_half_hour(at(23, 45)), t::local_to_utc(d + 1, 0));
    }

    // spec: CAL-008
    #[test]
    fn double_click_creates_events_in_every_view() {
        let (store, _) = store_with_samples("dbl");
        let mut h = H { ctx: egui::Context::default(), t: 0.0, actions: vec![] };
        let day = t::today() + 3;
        let local = |s: &str| -> (i64, i64) { t::utc_to_local(s.strip_prefix("new:").unwrap().parse().unwrap()) };
        // month, all sizes: that day, at the next half hour of the current time
        for size in [Size::Small, Size::Medium, Size::Large] {
            let mut h = H { ctx: egui::Context::default(), t: 0.0, actions: vec![] };
            let mut v = CalendarView::new(size);
            v.anchor = day;
            h.actions.clear();
            h.frame(&mut v, &store, true, vec![]);
            let r = H::rect(&v, &format!("day:{day}"));
            h.dblclick_at(&mut v, &store, true, Pos2::new(r.center().x, r.min.y + 4.0));
            assert_eq!(h.actions.len(), 1, "{size:?}: {:?}", h.actions);
            let (d, secs) = local(&h.actions[0]);
            assert!(d == day || d == day + 1, "{size:?}");
            assert_eq!(secs % 1800, 0);
        }
        // day view: at the clicked time, rounded up to the next half hour
        let mut v = CalendarView::new(Size::Large);
        v.mode = Mode::Day;
        v.anchor = day;
        h.actions.clear();
        h.frame(&mut v, &store, true, vec![]);
        h.frame(&mut v, &store, true, vec![]);
        let g = H::rect(&v, "hours");
        let hour_h = g.height() / 24.0;
        let y = g.min.y + 9.25 * hour_h; // 09:15
        assert!(y < 880.0 && y > 0.0, "the 9 o'clock row is on screen: {y}");
        h.dblclick_at(&mut v, &store, true, Pos2::new(g.max.x - 20.0, y));
        assert_eq!(h.actions.len(), 1, "{:?}", h.actions);
        assert_eq!(local(&h.actions[0]), (day, 9 * 3600 + 30 * 60));
        // week view: the column decides the day
        let mut h = H { ctx: egui::Context::default(), t: 0.0, actions: vec![] };
        let mut v = CalendarView::new(Size::Medium);
        v.mode = Mode::Week;
        v.anchor = day;
        h.actions.clear();
        h.frame(&mut v, &store, true, vec![]);
        h.frame(&mut v, &store, true, vec![]);
        let g = H::rect(&v, "hours");
        let col = H::rect(&v, &format!("day:{day}"));
        let y = g.min.y + 11.6 * g.height() / 24.0; // 11:36
        h.dblclick_at(&mut v, &store, true, Pos2::new(col.center().x, y));
        assert_eq!(local(&h.actions[0]), (day, 12 * 3600));
        // a single click on an empty spot does nothing
        h.actions.clear();
        h.click_at(&mut v, &store, true, Pos2::new(col.center().x, y));
        assert!(h.actions.is_empty());
    }

    #[test]
    fn empty_store_offers_to_add_a_calendar() {
        let store = CalendarStore::new(vec![], std::env::temp_dir().join("shale-calui-empty"));
        let mut v = CalendarView::new(Size::Small);
        let mut h = H { ctx: egui::Context::default(), t: 0.0, actions: vec![] };
        h.frame(&mut v, &store, false, vec![]);
        h.click(&mut v, &store, false, "add-calendar");
        assert_eq!(h.actions, vec!["settings".to_string()]);
        let _ = CalendarSource { id: String::new(), name: String::new(), color: String::new(), enabled: true, sync_minutes: 15, kind: SourceKind::Local { file: Default::default() } };
    }

    #[test]
    fn texts() {
        let d = days_from_civil(2026, 11, 20);
        let o = Occ { source: 0, event: 0, start: d * 86_400, end: (d + 3) * 86_400, all_day: true };
        assert_eq!(when_text(&o), "Fri, 20 Nov 2026 – Sun, 22 Nov 2026 (3 days)");
        assert_eq!(rrule_text("FREQ=WEEKLY;BYDAY=MO,WE;COUNT=6"), "weekly on Mon, Wed, 6 times");
        assert_eq!(rrule_text("FREQ=MONTHLY;INTERVAL=2"), "every 2 months");
    }
}
