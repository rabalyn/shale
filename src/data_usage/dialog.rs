//! The Data usage dialog (USE-001…USE-008) and the retention fields it shares with Settings
//! (USE-020, USE-021).

use super::{human_age, human_bytes, scan, CacheId, DataUsage};
use crate::app::muted;
use crate::config::CacheRetention;
use crate::theme;
use eframe::egui::{self, RichText, Ui};
use std::ops::RangeInclusive;
use std::path::PathBuf;

/// What the person asked for in the dialog; the window does it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    Refresh,
    /// clear (or rebuild) a cache; asked first where USE-008 says so
    Clear(CacheId),
    /// a retention setting of this cache changed (save the settings, prune later)
    Changed(CacheId),
    /// open a vault file (USE-003)
    Open(PathBuf),
    /// show a vault folder in the Files panel
    ShowFolder(PathBuf),
}

/// One retention setting.
pub struct Field {
    pub label: &'static str,
    pub unit: &'static str,
    /// what `None` is called
    pub none: &'static str,
    pub range: RangeInclusive<u32>,
    pub get: fn(&mut CacheRetention) -> &mut Option<u32>,
}

const DAYS: RangeInclusive<u32> = 1..=3650;
const MB: RangeInclusive<u32> = 1..=100_000;

/// The retention settings of a cache (none for the caches kept current).
pub fn fields(id: CacheId) -> Vec<Field> {
    match id {
        CacheId::SocialPictures => vec![Field { label: "Keep for", unit: "days", none: "forever", range: DAYS, get: |r| &mut r.picture_days }, Field { label: "At most", unit: "MB", none: "no limit", range: MB, get: |r| &mut r.picture_mb }],
        CacheId::PostIndex => vec![
            Field { label: "Keep for", unit: "days", none: "forever", range: DAYS, get: |r| &mut r.post_days },
            Field { label: "At most", unit: "posts per account", none: "no limit", range: 1..=1_000_000, get: |r| &mut r.posts_per_account },
        ],
        CacheId::MailBackups => vec![Field { label: "Keep the newest", unit: "backups", none: "all", range: 1..=100, get: |r| &mut r.mail_backups }],
        CacheId::MailBroken => vec![Field { label: "Keep for", unit: "days", none: "forever", range: DAYS, get: |r| &mut r.broken_days }],
        CacheId::DebugLog => vec![Field { label: "At most", unit: "MB", none: "no limit", range: MB, get: |r| &mut r.debug_log_mb }],
        CacheId::MailIndex | CacheId::TagWords => vec![],
    }
}

/// Bring every setting into its range (a hand-edited file, USE-021).
pub fn sanitize(r: &mut CacheRetention) {
    for id in CacheId::ALL {
        for f in fields(id) {
            if let Some(v) = (f.get)(r) {
                *v = (*v).clamp(*f.range.start(), *f.range.end());
            }
        }
    }
}

/// The retention fields of one cache; returns whether one changed.
pub fn retention_ui(ui: &mut Ui, id: CacheId, r: &mut CacheRetention) -> bool {
    if id.kept_current() {
        ui.label(muted("Kept current: no retention setting").size(12.0));
        return false;
    }
    let mut changed = false;
    for f in fields(id) {
        let v = (f.get)(r);
        ui.horizontal(|ui| {
            ui.label(f.label);
            if let Some(n) = v.as_mut() {
                changed |= ui.add(egui::DragValue::new(n).range(f.range.clone()).speed(1.0)).changed();
                ui.label(f.unit);
            }
            let mut unlimited = v.is_none();
            if ui.checkbox(&mut unlimited, f.none).changed() {
                *v = (!unlimited).then(|| default_of(f.get).clamp(*f.range.start(), *f.range.end()));
                changed = true;
            }
        });
    }
    changed
}

/// The default value of a field (where switching a limit back on starts).
fn default_of(get: fn(&mut CacheRetention) -> &mut Option<u32>) -> u32 {
    let mut d = CacheRetention::default();
    get(&mut d).unwrap_or(1)
}

fn size_row(ui: &mut Ui, l: &scan::Line) {
    let name = ui.label(&l.label);
    if let Some(p) = &l.path {
        name.on_hover_text(p.display().to_string());
    }
    ui.label(muted(format!("{} file{}", l.files, if l.files == 1 { "" } else { "s" })));
    ui.label(human_bytes(l.bytes));
    ui.end_row();
    if let Some(p) = &l.problem {
        ui.label("");
        ui.label(RichText::new(p).color(theme::LINK_MISSING).size(12.0));
        ui.end_row();
    }
}

fn saved_ui(ui: &mut Ui, u: &DataUsage, out: &mut Vec<Ask>) {
    ui.label(RichText::new("Saved").strong());
    ui.label(muted("Your notes, files and mail. Nothing here is deleted from this dialog.").size(12.0));
    let saved = match &u.saved {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(muted("counting…"));
            });
            return;
        }
        Some(Err(e)) => {
            ui.label(RichText::new(format!("Could not count: {e}")).color(theme::LINK_MISSING));
            return;
        }
        Some(Ok(s)) => s,
    };
    if u.places.vault.is_some() {
        ui.label(muted("In the vault"));
        egui::Grid::new("usage-vault").num_columns(3).striped(true).show(ui, |ui| {
            for l in &saved.vault {
                size_row(ui, l);
            }
        });
        egui::CollapsingHeader::new(format!("Largest {} files", scan::LARGEST)).id_salt("usage-largest-files").show(ui, |ui| {
            for (rel, n) in &saved.largest_files {
                ui.horizontal(|ui| {
                    if ui.link(rel.to_string_lossy()).on_hover_text("Open").clicked() {
                        out.push(Ask::Open(rel.clone()));
                    }
                    ui.label(muted(human_bytes(*n)));
                });
            }
        });
        egui::CollapsingHeader::new(format!("Largest {} folders", scan::LARGEST)).id_salt("usage-largest-folders").show(ui, |ui| {
            for (rel, n) in &saved.largest_folders {
                ui.horizontal(|ui| {
                    if ui.link(rel.to_string_lossy()).on_hover_text("Show in the file list").clicked() {
                        out.push(Ask::ShowFolder(rel.clone()));
                    }
                    ui.label(muted(human_bytes(*n)));
                });
            }
        });
    }
    ui.add_space(4.0);
    ui.label(muted("Outside the vault"));
    egui::Grid::new("usage-outside").num_columns(3).striped(true).show(ui, |ui| {
        for l in &saved.outside {
            size_row(ui, l);
        }
    });
}

/// The Clear button of a cache was clicked: clear at once, or ask first (USE-008).
pub fn clear_clicked(u: &mut DataUsage, id: CacheId) -> Option<Ask> {
    if id.confirm_text().is_some() {
        u.confirm = Some(id);
        None
    } else {
        Some(Ask::Clear(id))
    }
}

fn cache_ui(ui: &mut Ui, u: &mut DataUsage, r: &mut CacheRetention, now: i64, out: &mut Vec<Ask>) {
    ui.label(RichText::new("Caches").strong());
    ui.label(muted("Shale can build these again or fetch them again.").size(12.0));
    for id in CacheId::ALL {
        ui.add_space(6.0);
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(id.name()).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let running = u.running(id);
                    let b = ui.add_enabled(!running && u.confirm.is_none(), egui::Button::new(id.clear_label()));
                    if b.clicked() {
                        out.extend(clear_clicked(u, id));
                    }
                    if running {
                        ui.spinner();
                    }
                });
            });
            ui.label(muted(u.places.location(id)).size(11.0));
            let line = match u.stats.get(&id) {
                None => "counting…".to_string(),
                Some(Err(e)) => format!("could not count: {e}"),
                Some(Ok(s)) => {
                    let mut t = format!("{} · {} entr{}", human_bytes(s.bytes), s.entries, if s.entries == 1 { "y" } else { "ies" });
                    if let Some(o) = s.oldest {
                        t.push_str(&format!(" · oldest {}", human_age(now - o)));
                    }
                    t
                }
            };
            ui.label(line);
            if retention_ui(ui, id, r) {
                out.push(Ask::Changed(id));
            }
            ui.label(muted(u.record.text(id, now)).size(12.0));
        });
    }
}

/// The dialog's content; returns what was asked for.
pub fn show(ui: &mut Ui, u: &mut DataUsage, r: &mut CacheRetention) -> Vec<Ask> {
    let mut out = vec![];
    let now = crate::calendar::time::now();
    saved_ui(ui, u, &mut out);
    ui.add_space(10.0);
    ui.separator();
    cache_ui(ui, u, r, now, &mut out);
    out
}

/// The footer (NAV-028), below the scrolling content: the question of a Clear that asks first
/// (USE-008), so its answer is in reach wherever the content is scrolled to, and Refresh with
/// the total. Returns what was asked for.
pub fn footer(ui: &mut Ui, u: &mut DataUsage) -> Vec<Ask> {
    let mut out = vec![];
    if let Some(id) = u.confirm {
        if let Some(t) = id.confirm_text() {
            ui.label(RichText::new(format!("{}: {t}", id.name())).color(theme::LINK_MISSING));
        }
        ui.horizontal(|ui| {
            if ui.button(format!("Yes, {}", id.clear_label().to_lowercase())).clicked() {
                u.confirm = None;
                out.push(Ask::Clear(id));
            }
            if ui.button("Cancel").clicked() {
                u.confirm = None;
            }
        });
    }
    ui.horizontal(|ui| {
        if ui.add_enabled(u.saved.is_some(), egui::Button::new("Refresh")).on_hover_text("Count again").clicked() {
            out.push(Ask::Refresh);
        }
        if let Some(t) = u.total() {
            ui.label(muted(format!("Caches: {}", human_bytes(t))));
        }
    });
    out
}
