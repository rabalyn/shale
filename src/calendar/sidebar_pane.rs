//! Calendar as a view of the right sidebar (LAY-033, ADR-0038): a compact companion of the
//! Calendar tab, not its month, week and day views squeezed into a narrow column. It is a small
//! month (‹ › to change it, today marked, a dot under the days with events) and under it the
//! agenda of the next [`AGENDA_DAYS`] days from the chosen day, each day with its events (all-day
//! events first, then by time). A click on a day of the month moves the agenda to that day; a
//! click on an event asks the window to show it in the Calendar tab.
//!
//! The view state (the shown month, the chosen day, the scroll) is the pane's own, so the
//! sidebar can show this month while the tab shows another one (LAY-031). The data is the tab's:
//! the one `CalendarStore` with its sources, its cache of occurrences and its sync, so nothing
//! is read or fetched twice.

use crate::app::muted;
use crate::calendar::store::{CalendarStore, Occ};
use crate::calendar::time::{self as t, civil_from_days, days_from_civil, MONTHS, WEEKDAYS};
use crate::calendar::ui::{parse_hex, week_start_of};
use crate::theme;
use eframe::egui::{self, Align2, Color32, CornerRadius, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

/// How many days the agenda lists, from the chosen day on (the chosen day is the first).
pub(crate) const AGENDA_DAYS: i64 = 14;

/// The height of a day of the month and of an event of the agenda, in points.
const DAY_H: f32 = 24.0;
const EVENT_H: f32 = 22.0;
/// The width of the column of times in the agenda.
const TIME_W: f32 = 46.0;

/// The state of the Calendar view of the right sidebar. It is not saved: a new window shows the
/// month of today with today chosen.
#[derive(Default)]
pub(crate) struct CalendarPane {
    /// the first day of the shown month; None: the month of today
    pub month: Option<i64>,
    /// the chosen day, where the agenda starts; None: today
    pub selected: Option<i64>,
    /// how far the pane is scrolled, in points (the last frame's)
    pub scroll: f32,
    /// controls drawn in the last frame, by name (for the UI tests)
    pub rects: Vec<(String, Rect)>,
}

impl CalendarPane {
    /// The first day of the month the pane shows.
    pub fn shown_month(&self) -> i64 {
        self.month.unwrap_or_else(|| {
            let (y, m, _) = civil_from_days(t::today());
            days_from_civil(y, m, 1)
        })
    }

    /// The day the agenda starts at.
    pub fn chosen_day(&self) -> i64 {
        self.selected.unwrap_or_else(t::today)
    }

    fn step_month(&mut self, dir: i32) {
        let (y, m, _) = civil_from_days(self.shown_month());
        let (y, m) = t::add_months(y, m, dir);
        self.month = Some(days_from_civil(y, m, 1));
    }

    /// Draw the pane into the free space of `ui`. Returns the event that was clicked: the window
    /// shows it in the Calendar tab.
    pub fn show(&mut self, ui: &mut Ui, store: &CalendarStore, monday: bool) -> Option<Occ> {
        self.rects.clear();
        if store.sources.is_empty() {
            ui.label(muted("No calendar yet. Choose Calendar in the ribbon and add one in its settings."));
            return None;
        }
        let mut rects = std::mem::take(&mut self.rects);
        let mut clicked = None;
        let out = egui::ScrollArea::vertical().id_salt("calendar-pane").auto_shrink([false, false]).show(ui, |ui| {
            self.month_grid(ui, store, monday, &mut rects);
            ui.add_space(6.0);
            ui.separator();
            clicked = self.agenda(ui, store, &mut rects);
        });
        self.scroll = out.state.clamped_offset().y;
        self.rects = rects;
        clicked
    }

    /// The title line with ‹ › and Today, the weekdays and the days of the shown month.
    fn month_grid(&mut self, ui: &mut Ui, store: &CalendarStore, monday: bool, rects: &mut Vec<(String, Rect)>) {
        let first = self.shown_month();
        let (y, m, _) = civil_from_days(first);
        ui.horizontal(|ui| {
            let prev = ui.small_button("‹").on_hover_text("Previous month");
            rects.push(("calpane:prev".into(), prev.rect));
            let next = ui.small_button("›").on_hover_text("Next month");
            rects.push(("calpane:next".into(), next.rect));
            ui.label(RichText::new(format!("{} {y}", MONTHS[m as usize - 1])).strong().color(theme::palette().text_strong));
            let today = ui.small_button("Today").on_hover_text("Show this month and today's agenda");
            rects.push(("calpane:today".into(), today.rect));
            if prev.clicked() {
                self.step_month(-1);
            }
            if next.clicked() {
                self.step_month(1);
            }
            if today.clicked() {
                self.month = None;
                self.selected = None;
            }
        });
        let start = week_start_of(first, monday);
        let rows = ((first - start + t::days_in_month(y, m) as i64) + 6) / 7;
        let last = start + rows * 7 - 1;
        let busy: std::collections::HashSet<i64> = store.occurrences(start, last).iter().flat_map(|o| o.start_day().max(start)..=o.end_day().min(last)).collect();
        let w = ui.available_width();
        let cw = w / 7.0;
        let (hr, _) = ui.allocate_exact_size(Vec2::new(w, 16.0), Sense::hover());
        for i in 0..7 {
            let wd = if monday { i } else { (i + 6) % 7 };
            ui.painter().text(Pos2::new(hr.min.x + cw * (i as f32 + 0.5), hr.center().y), Align2::CENTER_CENTER, &WEEKDAYS[wd][..2], crate::fonts::font(11.0), theme::palette().muted);
        }
        let today = t::today();
        let chosen = self.chosen_day();
        let (grid, _) = ui.allocate_exact_size(Vec2::new(w, DAY_H * rows as f32), Sense::hover());
        let mut pick = None;
        for i in 0..rows * 7 {
            let d = start + i;
            let r = Rect::from_min_size(grid.min + Vec2::new(cw * (i % 7) as f32, DAY_H * (i / 7) as f32), Vec2::new(cw, DAY_H)).shrink(1.0);
            let resp = ui.interact(r, ui.scope_id().with(("calendar-pane-day", d)), Sense::click());
            rects.push((format!("calpane:day:{d}"), r));
            let p = ui.painter();
            if resp.hovered() {
                p.rect_filled(r, CornerRadius::same(4), theme::palette().bg_hover);
            }
            if d == chosen {
                p.rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, theme::palette().accent), egui::StrokeKind::Inside);
            }
            let num_pos = Pos2::new(r.center().x, r.min.y + 9.0);
            if d == today {
                p.circle_filled(num_pos, 8.0, theme::palette().accent);
                rects.push((format!("calpane:today:{d}"), r));
            }
            let in_month = civil_from_days(d).1 == m;
            let colour = if d == today {
                Color32::WHITE
            } else if in_month {
                theme::palette().text
            } else {
                theme::palette().faint
            };
            p.text(num_pos, Align2::CENTER_CENTER, civil_from_days(d).2.to_string(), crate::fonts::font(11.5), colour);
            if busy.contains(&d) {
                p.circle_filled(Pos2::new(r.center().x, r.max.y - 3.5), 2.0, theme::palette().accent);
                rects.push((format!("calpane:busy:{d}"), r));
            }
            if resp.clicked() {
                pick = Some(d);
            }
        }
        if let Some(d) = pick {
            self.selected = Some(d);
            // a day of the neighbouring month brings that month
            if civil_from_days(d).1 != m {
                let (y, m, _) = civil_from_days(d);
                self.month = Some(days_from_civil(y, m, 1));
            }
        }
    }

    /// The events of the [`AGENDA_DAYS`] days from the chosen day, a heading per day with events.
    fn agenda(&mut self, ui: &mut Ui, store: &CalendarStore, rects: &mut Vec<(String, Rect)>) -> Option<Occ> {
        let (from, to) = (self.chosen_day(), self.chosen_day() + AGENDA_DAYS - 1);
        let occs = store.occurrences(from, to);
        if occs.is_empty() {
            let text = if store.is_loading() { "Loading the calendars…".to_string() } else { format!("Nothing in the next {AGENDA_DAYS} days.") };
            ui.label(muted(text));
            return None;
        }
        let mut clicked = None;
        let today = t::today();
        for d in from..=to {
            // all-day events first, then by start
            let mut day: Vec<&Occ> = occs.iter().filter(|o| o.start_day() <= d && o.end_day() >= d).collect();
            if day.is_empty() {
                continue;
            }
            day.sort_by_key(|o| (!o.all_day, o.start));
            let when = if d == today {
                "Today · "
            } else if d == today + 1 {
                "Tomorrow · "
            } else {
                ""
            };
            ui.add_space(4.0);
            ui.label(RichText::new(format!("{when}{}", t::long_date(d))).size(crate::fonts::px(12.5)).color(theme::palette().text_strong));
            for o in day {
                let Some(e) = store.event(o) else { continue };
                let summary = if e.summary.is_empty() { "(no title)".to_string() } else { e.summary.clone() };
                let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), EVENT_H), Sense::click());
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                let colour = parse_hex(&store.sources[o.source].color);
                let p = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
                if resp.hovered() {
                    p.rect_filled(r, CornerRadius::same(3), theme::palette().bg_hover);
                }
                p.rect_filled(Rect::from_min_size(r.min + Vec2::new(0.0, 2.0), Vec2::new(3.0, r.height() - 4.0)), CornerRadius::same(1), colour);
                let time = if o.all_day {
                    "all day".to_string()
                } else if o.start_day() < d {
                    "…".to_string()
                } else {
                    t::hhmm(t::utc_to_local(o.start).1)
                };
                p.text(r.left_center() + Vec2::new(8.0, 0.0), Align2::LEFT_CENTER, time, crate::fonts::font(11.0), theme::palette().muted);
                p.text(r.left_center() + Vec2::new(8.0 + TIME_W, 0.0), Align2::LEFT_CENTER, &summary, crate::fonts::font(12.0), theme::palette().text);
                rects.push((format!("calpane:event:{summary}:{d}"), r));
                if resp.clicked() {
                    clicked = Some(o.clone());
                }
            }
        }
        clicked
    }
}
