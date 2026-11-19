//! The front matter in the reading view (EDIT-063, EDIT-064, CFG-050): a card by the type of the
//! note, a folded line or the raw lines, with a ▸/▾ toggle for all raw properties. The model is
//! `properties`; this file only draws it.

use crate::config::PropertiesView;
use crate::properties::{self, Card, Kind, Person};
use crate::reader::ReaderAction;
use crate::theme;
use crate::vault::Vault;
use eframe::egui::{self, Align, CornerRadius, Layout, RichText, Sense, Ui};
use std::path::Path;

/// Draw the front matter of `text`; `raw` are its property lines (between the two `---`).
pub fn show(ui: &mut Ui, text: &str, raw: &[&str], view: PropertiesView, vault: &Vault, rel: &Path, out: &mut Option<ReaderAction>) {
    let card = Card::parse(text);
    let view = if card.is_none() { PropertiesView::Raw } else { view };
    egui::Frame::new().fill(theme::BG_SECONDARY).corner_radius(CornerRadius::same(6)).inner_margin(8).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let open_id = egui::Id::unique(("shale-properties-open", rel));
        let open: bool = ui.ctx().data(|d| d.get_temp(open_id)).unwrap_or(false);
        let mut toggled = false;
        match (view, &card) {
            (PropertiesView::Raw, _) | (_, None) => raw_lines(ui, raw),
            (PropertiesView::Folded, Some(_)) => {
                ui.horizontal(|ui| {
                    toggled = toggle(ui, open);
                    ui.label(RichText::new(properties::summary(text).unwrap_or_else(|| "Properties".into())).color(theme::MUTED));
                });
            }
            (PropertiesView::Card, Some(c)) => {
                match c.kind {
                    Kind::Email => email(ui, c, vault, rel, out),
                    Kind::Event => event(ui, c, vault, rel, out),
                    Kind::Word => word(ui, c),
                    Kind::Generic => generic(ui, c),
                }
                tags(ui, c);
                ui.horizontal(|ui| {
                    toggled = toggle(ui, open);
                    ui.label(RichText::new("All properties").small().color(theme::MUTED));
                });
            }
        }
        if toggled {
            ui.ctx().data_mut(|d| d.insert_temp(open_id, !open));
        }
        if open && view != PropertiesView::Raw && card.is_some() {
            ui.add_space(4.0);
            raw_lines(ui, raw);
        }
    });
}

/// The ▸/▾ button; true when it was clicked.
fn toggle(ui: &mut Ui, open: bool) -> bool {
    let r = ui.add(egui::Label::new(RichText::new(if open { "▾" } else { "▸" }).color(theme::MUTED)).sense(Sense::click()));
    r.on_hover_text(if open { "Hide all properties" } else { "Show all properties" }).clicked()
}

fn raw_lines(ui: &mut Ui, raw: &[&str]) {
    ui.label(RichText::new("Properties").small().color(theme::MUTED));
    for l in raw {
        ui.label(RichText::new(*l).monospace().color(theme::TEXT));
    }
}

/// People of a property value: a name that is a note is a link (EDIT-064), the address is the tooltip.
fn people(ui: &mut Ui, value: &str, strong: bool, vault: &Vault, rel: &Path, out: &mut Option<ReaderAction>) {
    let list = properties::people_in(value);
    for (i, p) in list.iter().enumerate() {
        if i > 0 {
            ui.label(RichText::new(",").color(theme::MUTED));
        }
        person(ui, p, strong, vault, rel, out);
    }
}

fn person(ui: &mut Ui, p: &Person, strong: bool, vault: &Vault, rel: &Path, out: &mut Option<ReaderAction>) {
    let known = !p.name.is_empty() && vault.exists(&p.name, Some(rel));
    let mut t = RichText::new(p.label());
    t = if known {
        t.color(theme::LINK)
    } else if strong {
        t.color(theme::TEXT_STRONG)
    } else {
        t.color(theme::TEXT)
    };
    if strong {
        t = t.strong();
    }
    let mut r = ui.add(egui::Label::new(t).sense(if known { Sense::click() } else { Sense::hover() }));
    if !p.email.is_empty() {
        r = r.on_hover_text(&p.email);
    }
    if known {
        r = r.on_hover_cursor(egui::CursorIcon::PointingHand);
        if r.clicked() || r.middle_clicked() {
            *out = Some(ReaderAction::OpenLink { target: p.name.clone(), heading: None, new_tab: r.middle_clicked() || ui.input(|i| i.modifiers.command) });
        }
    }
}

/// A date of a card (EDIT-108): `text` is drawn as a link that shows the day of `stamp`
/// (`2026-11-12 14:30`) in the calendar; a stamp that is no date is plain text.
fn date(ui: &mut Ui, text: &str, stamp: &str, strong: bool, out: &mut Option<ReaderAction>) {
    let Some(day) = crate::calendar::time::parse_ymd(properties::date_part(stamp)) else {
        let t = RichText::new(text);
        ui.label(if strong { t.strong().color(theme::TEXT_STRONG) } else { t.color(theme::MUTED) });
        return;
    };
    let mut t = RichText::new(text).color(theme::LINK);
    if strong {
        t = t.strong();
    }
    let r = ui.add(egui::Label::new(t).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Show this day in the calendar");
    if r.clicked() {
        *out = Some(ReaderAction::OpenDay(day));
    }
}

fn muted(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::MUTED));
}

/// A row of people after a muted word: `to Bob, Carol`.
fn people_row(ui: &mut Ui, word: &str, value: &str, vault: &Vault, rel: &Path, out: &mut Option<ReaderAction>) {
    ui.horizontal_wrapped(|ui| {
        muted(ui, word);
        people(ui, value, false, vault, rel, out);
    });
}

fn email(ui: &mut Ui, c: &Card, vault: &Vault, rel: &Path, out: &mut Option<ReaderAction>) {
    ui.horizontal(|ui| {
        if let Some(f) = c.get("from") {
            people(ui, f, true, vault, rel, out);
        }
        if let Some(d) = c.get("date") {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| date(ui, d, d, false, out));
        }
    });
    if let Some(t) = c.get("to") {
        people_row(ui, "to", t, vault, rel, out);
    }
    if let Some(t) = c.get("cc") {
        people_row(ui, "cc", t, vault, rel, out);
    }
    if let Some(s) = c.get("subject") {
        ui.label(RichText::new(s).color(theme::TEXT));
    }
}

/// A small rounded badge.
fn chip(ui: &mut Ui, text: &str, fg: egui::Color32, bg: egui::Color32) {
    egui::Frame::new().fill(bg).corner_radius(CornerRadius::same(8)).inner_margin(egui::Margin::symmetric(7, 1)).show(ui, |ui| {
        ui.label(RichText::new(text).small().color(fg));
    });
}

fn tags(ui: &mut Ui, c: &Card) {
    let tags = c.tags();
    if tags.is_empty() {
        return;
    }
    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        for t in tags {
            chip(ui, &format!("#{t}"), theme::TAG, theme::TAG_BG);
        }
    });
}

fn event(ui: &mut Ui, c: &Card, vault: &Vault, rel: &Path, out: &mut Option<ReaderAction>) {
    ui.horizontal_wrapped(|ui| {
        if let Some(w) = c.when() {
            date(ui, &w, c.get("start").unwrap_or_default(), true, out);
        }
        if let Some(r) = c.get("repeats") {
            chip(ui, &format!("repeats {r}"), theme::TEXT, theme::BG_HOVER);
        }
    });
    if let Some(l) = c.get("location") {
        ui.horizontal_wrapped(|ui| {
            muted(ui, "at");
            ui.label(RichText::new(l).color(theme::TEXT));
        });
    }
    for (word, key) in [("organizer", "organizer"), ("with", "attendees")] {
        let vals = c.values(key).join(", ");
        if !vals.trim().is_empty() {
            people_row(ui, word, &vals, vault, rel, out);
        }
    }
    if let Some(cal) = c.get("calendar") {
        ui.label(RichText::new(cal).small().color(theme::MUTED));
    }
}

fn word(ui: &mut Ui, c: &Card) {
    ui.horizontal_wrapped(|ui| {
        muted(ui, "Word document");
        if let Some(p) = c.page_chip() {
            chip(ui, &p, theme::TEXT, theme::BG_HOVER);
        }
    });
    for (k, v) in c.rows() {
        row(ui, &k, &v);
    }
}

fn generic(ui: &mut Ui, c: &Card) {
    for (k, v) in c.rows() {
        row(ui, &k, &v);
    }
}

fn row(ui: &mut Ui, key: &str, value: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("{key}:")).color(theme::MUTED));
        ui.label(RichText::new(value).color(theme::TEXT));
    });
}
