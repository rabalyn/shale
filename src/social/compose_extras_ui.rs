//! The parts of the compose box that came after the text and the pictures (#589): the language
//! (SOC-065), a poll (SOC-061), a time to send the post (SOC-063) and a video (SOC-066). The box
//! itself is `compose_ui.rs`; these are drawn into it. Every control puts its rectangle into
//! `rects` under a `compose:…` key, as the controls of the box do (tests click them there).

use super::actions::InstanceInfo;
use super::compose::Compose;
use super::feed_ui::short;
use super::language;
use super::poll::ComposePoll;
use super::schedule::{when_text, ScheduleInput};
use super::Network;
use crate::clickable::ComboBoxPointer;
use crate::theme;
use eframe::egui::{self, Rect, RichText, Ui};

fn muted(t: impl Into<String>) -> RichText {
    RichText::new(t).size(crate::fonts::px(12.0)).color(theme::palette().muted)
}

/// The language of the post: a drop-down with the languages of `language::LANGUAGES` and "No language".
pub(super) fn language_picker(ui: &mut Ui, c: &mut Compose, rects: &mut Vec<(String, Rect)>) {
    ui.horizontal(|ui| {
        ui.label(muted("Language"));
        let combo = egui::ComboBox::from_id_salt("compose-language").selected_text(language::name(&c.language)).show_pointer(ui, |ui| {
            crate::clickable::pointer(ui.selectable_value(&mut c.language, String::new(), language::name("")));
            for (code, _) in language::LANGUAGES {
                crate::clickable::pointer(ui.selectable_value(&mut c.language, code.to_string(), language::name(code)));
            }
        });
        rects.push(("compose:language".into(), combo.response.rect));
    });
}

/// The poll of a Mastodon post: add it, its options (2 up to what the server allows), how long it
/// runs, multiple choice, hidden totals. A poll and pictures or a video exclude each other.
pub(super) fn poll_section(ui: &mut Ui, c: &mut Compose, info: &InstanceInfo, rects: &mut Vec<(String, Rect)>) {
    let media = !c.pictures.is_empty() || c.video.is_some();
    let Some(poll) = c.poll.as_mut() else {
        ui.horizontal(|ui| {
            let b = ui.add_enabled(!media, egui::Button::new("Add poll")).on_hover_text("Ask a question with up to a few answers");
            rects.push(("compose:add-poll".into(), b.rect));
            if media {
                let r = ui.label(muted("A poll and pictures or a video cannot be in one post on Mastodon."));
                rects.push(("compose:poll-note".into(), r.rect));
            }
            if b.clicked() {
                let mut p = ComposePoll::default();
                p.fit(&info.poll);
                c.poll = Some(p);
            }
        });
        return;
    };
    let limits = info.poll;
    let mut remove_poll = false;
    super::feed_ui::inset(ui, |ui| {
        ui.label(muted("Poll (it replaces pictures and videos: a post has one or the other)"));
        let mut drop_option = None;
        let many = poll.options.len() > 2;
        for (i, o) in poll.options.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(o).char_limit(limits.max_chars).hint_text(format!("Option {}", i + 1)).desired_width(ui.available_width() - 34.0));
                rects.push((format!("compose:poll-option:{i}"), r.rect));
                let b = ui.add_enabled(many, egui::Button::new("×").small()).on_hover_text("Remove this option");
                rects.push((format!("compose:poll-remove:{i}"), b.rect));
                if b.clicked() {
                    drop_option = Some(i);
                }
            });
        }
        if let Some(i) = drop_option {
            poll.options.remove(i);
        }
        ui.horizontal(|ui| {
            let b = ui.add_enabled(poll.options.len() < limits.max_options, egui::Button::new("Add option").small());
            rects.push(("compose:poll-add-option".into(), b.rect));
            if b.clicked() {
                poll.options.push(String::new());
            }
            ui.label(muted(format!("{} of at most {} options · up to {} characters each", poll.options.len(), limits.max_options, limits.max_chars)));
        });
        ui.horizontal(|ui| {
            ui.label(muted("Open for"));
            let all = ComposePoll::durations(&limits);
            let shown = all.iter().find(|(s, _)| *s == poll.expires_in).map(|(_, w)| w.clone()).unwrap_or_default();
            let combo = egui::ComboBox::from_id_salt("compose-poll-duration").selected_text(shown).show_pointer(ui, |ui| {
                for (secs, words) in &all {
                    crate::clickable::pointer(ui.selectable_value(&mut poll.expires_in, *secs, words));
                }
            });
            rects.push(("compose:poll-duration".into(), combo.response.rect));
        });
        let r = crate::clickable::pointer(ui.checkbox(&mut poll.multiple, "People may choose several options"));
        rects.push(("compose:poll-multiple".into(), r.rect));
        let r = crate::clickable::pointer(ui.checkbox(&mut poll.hide_totals, "Hide the totals until the poll ends"));
        rects.push(("compose:poll-hide".into(), r.rect));
        let b = ui.small_button("Remove poll");
        rects.push(("compose:poll-remove".into(), b.rect));
        remove_poll = b.clicked();
    });
    if remove_poll {
        c.poll = None;
    }
}

/// Sending the post later (Mastodon): the date and time in the local time zone, and the way to
/// the list of the scheduled posts. `list` is set when that was pressed.
pub(super) fn schedule_section(ui: &mut Ui, c: &mut Compose, monday_first: bool, list: &mut bool, rects: &mut Vec<(String, Rect)>) {
    ui.horizontal(|ui| {
        if c.schedule.is_none() {
            let b = ui.button("Schedule…").on_hover_text("Let the server send this post later");
            rects.push(("compose:schedule".into(), b.rect));
            if b.clicked() {
                c.schedule = Some(ScheduleInput::soon());
            }
        } else {
            let b = ui.button("Send now instead").on_hover_text("Do not schedule: post it when you press Post");
            rects.push(("compose:schedule-off".into(), b.rect));
            if b.clicked() {
                c.schedule = None;
            }
        }
        let b = ui.button("Scheduled posts…").on_hover_text("The posts you scheduled: see them and cancel one");
        rects.push(("compose:scheduled-list".into(), b.rect));
        *list = b.clicked();
    });
    let Some(s) = c.schedule.as_mut() else { return };
    ui.horizontal(|ui| {
        ui.label(muted("Send on"));
        // the date can be typed or picked in the calendar's month grid; the time is two numbers (click one to type)
        let r = ui.add(egui::TextEdit::singleline(&mut s.date).hint_text("2026-11-23").desired_width(86.0));
        rects.push(("compose:schedule-date".into(), r.rect));
        crate::calendar::dialog::date_picker(ui, "schedule-day", &mut s.date, monday_first, rects);
        let secs = crate::calendar::dialog::parse_time(&s.time);
        let (mut h, mut m) = secs.map_or((12, 0), |v| (v / 3600, v % 3600 / 60));
        let hour = ui.add(egui::DragValue::new(&mut h).range(0..=23).custom_formatter(|n, _| format!("{:02}", n as i64)));
        rects.push(("compose:schedule-hour".into(), hour.rect));
        ui.label(":");
        let minute = ui.add(egui::DragValue::new(&mut m).range(0..=59).custom_formatter(|n, _| format!("{:02}", n as i64)));
        rects.push(("compose:schedule-minute".into(), minute.rect));
        if hour.changed() || minute.changed() {
            s.time = format!("{h:02}:{m:02}");
        }
        ui.label(muted("your local time, at least 5 minutes ahead"));
    });
    let line = match s.parse(crate::calendar::time::now()) {
        Ok(at) => muted(format!("Will be sent {}", when_text(at))),
        Err(e) => RichText::new(e).size(crate::fonts::px(12.0)).color(theme::palette().link_missing),
    };
    let r = ui.label(line);
    rects.push(("compose:schedule-note".into(), r.rect));
}

/// The video of the post with its description and Remove.
pub(super) fn video_row(ui: &mut Ui, c: &mut Compose, sending: bool, network: Network, rects: &mut Vec<(String, Rect)>) {
    let Some(v) = c.video.as_mut() else { return };
    let mut remove = false;
    ui.horizontal(|ui| {
        let name = v.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        ui.label(muted(format!("Video {}", short(&name, 22))));
        let r = ui.add_enabled(!sending, egui::TextEdit::singleline(&mut v.alt).char_limit(if network == Network::Bluesky { 10_000 } else { 1500 }).hint_text("Describe the video (alt text)").desired_width(ui.available_width() - 80.0));
        rects.push(("compose:video-alt".into(), r.rect));
        let r = ui.add_enabled(!sending, egui::Button::new("Remove").small());
        rects.push(("compose:video-remove".into(), r.rect));
        remove = r.clicked();
    });
    if remove {
        c.video = None;
    }
}
