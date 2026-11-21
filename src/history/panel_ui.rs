//! The list of the versions of a note: a column in the margin of the note and a panel of the right
//! sidebar (HIST-001…HIST-007).

use super::labels::{full_time, group_by_day, size_hover, size_text, source_label, ticks, time_label};
use super::place::{self, Place};
use crate::app::{muted, SettingsTab, ShaleApp, View};
use crate::gitsync::history::Version;
use crate::layout::{icon_button, Icon};
use crate::theme;
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Sense, Ui, Vec2};

const ROW_H: f32 = 40.0;
const TICK: Vec2 = Vec2::new(6.0, 10.0);
const TICK_GAP: f32 = 2.0;

/// What a click in the list asked for.
enum Do {
    Open(Version),
    Restore(Version),
    Copy(Version),
}

impl ShaleApp {
    /// Where the history of the focused note is shown, given the width of the note area (HIST-001).
    pub(crate) fn history_place(&mut self, area_w: f32) -> Place {
        let note = matches!(self.tabbar.tabs.get(self.tabbar.active).map(|t| &t.view), Some(View::Note(_)));
        let wanted = self.cfg.history.show_panel && self.history_active();
        place::place(place::Facts { panes: self.panes.len(), note, readable: self.cfg.readable_width, area_w, wanted })
    }

    /// The column in the margin of the note (HIST-001): its header has the button that hides it.
    pub(crate) fn history_margin(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        let mut hide = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("History").strong().color(theme::palette().text_strong));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let b = icon_button(ui, Icon::Close, 20.0, false, "Hide the history");
                self.history.panel.rects.push(("hide".into(), b.rect));
                hide = b.clicked();
            });
        });
        if hide {
            self.cfg.history.show_panel = false;
            self.save_config();
        }
        ui.add_space(2.0);
        self.history_list(ui);
    }

    /// The panel of the right sidebar (HIST-002).
    pub(crate) fn history_panel_body(&mut self, ui: &mut Ui) {
        self.history.panel.rects.retain(|(k, _)| k == "hide");
        if self.history.panel.place == Some(Place::Margin) {
            ui.add_space(6.0);
            ui.label(muted("The history of this note is shown beside it."));
            let b = ui.button("Show it here");
            self.history.panel.rects.push(("show-here".into(), b.rect));
            if b.clicked() {
                self.cfg.history.show_panel = false;
                self.save_config();
            }
            return;
        }
        self.history_list(ui);
    }

    /// The list, or the reason there is none.
    fn history_list(&mut self, ui: &mut Ui) {
        let rel = match self.current_path().filter(|p| crate::gitsync::history::tracked(p)) {
            Some(rel) if self.vault.is_some() => rel,
            _ => {
                ui.label(muted("Open a note to see its history."));
                return;
            }
        };
        if let Some(why) = self.history_unavailable() {
            ui.label(muted(why));
            ui.add_space(4.0);
            let b = ui.button("Open Settings \u{2192} Sync");
            self.history.panel.rects.push(("settings".into(), b.rect));
            if b.clicked() {
                self.open_settings(SettingsTab::Sync);
            }
            return;
        }
        self.history.panel.want = Some(rel.clone());
        let now = crate::calendar::time::now();
        let this_device = crate::gitsync::host_name();
        let mut asked: Option<Do> = None;
        let mut deleted = false;
        let panel = &mut self.history.panel;
        let shown = panel.note.as_deref() == Some(rel.as_path());
        if let Some(e) = panel.error.clone() {
            theme::problem(ui, e);
        } else if panel.loading && !shown {
            ui.label(muted("Reading the history\u{2026}"));
        } else if shown && panel.versions.is_empty() {
            ui.label(muted("No versions yet. Shale keeps a version when you save a note, and when you stop typing."));
        }
        let selected = (self.modal.is_some() && !self.history.changes.from.is_empty()).then(|| self.history.changes.from.clone());
        let bottom = 28.0;
        let height = (ui.available_height() - bottom).max(60.0);
        egui::ScrollArea::vertical().id_salt("history-list").max_height(height).auto_shrink([false, false]).show(ui, |ui| {
            if !shown {
                return;
            }
            let versions = self.history.panel.versions.clone();
            for (day, idx) in group_by_day(&versions, now) {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(day).size(12.0).strong().color(theme::palette().muted));
                for i in idx {
                    let v = &versions[i];
                    let source = source_label(&versions, i, &this_device, now);
                    let resp = version_row(ui, v, &source, now, selected.as_deref() == Some(v.commit.as_str()));
                    self.history.panel.rects.push((format!("row-{i}"), resp.rect));
                    if resp.clicked() {
                        asked = Some(Do::Open(v.clone()));
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Restore this version").clicked() {
                            asked = Some(Do::Restore(v.clone()));
                            ui.close();
                        }
                        if ui.button("Copy text").clicked() {
                            asked = Some(Do::Copy(v.clone()));
                            ui.close();
                        }
                    });
                }
            }
        });
        ui.add_space(2.0);
        let b = ui.add(egui::Button::new(egui::RichText::new("Restore a deleted note\u{2026}").size(12.0)).frame(false));
        self.history.panel.rects.push(("deleted".into(), b.rect));
        deleted |= b.clicked();
        match asked {
            Some(Do::Open(v)) => self.open_history_changes(&rel, &v),
            Some(Do::Restore(v)) => self.history_restore_version(&rel, &v),
            Some(Do::Copy(v)) => self.history_copy_version(&v),
            None => {}
        }
        if deleted {
            self.open_history_deleted();
        }
    }
}

/// One row: the time, the change bar and the source in words (HIST-004…HIST-006).
fn version_row(ui: &mut Ui, v: &Version, source: &str, now: i64, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
    let p = theme::palette();
    if selected {
        ui.painter().rect_filled(rect, 4.0, p.bg_active);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.bg_hover);
    }
    let time = time_label(v.time, now);
    let text_x = rect.min.x + 6.0;
    ui.painter().text(Pos2::new(text_x, rect.min.y + 5.0), Align2::LEFT_TOP, &time, FontId::proportional(13.5), p.text);
    // the bar of five ticks at the right edge, and the counts as text before it
    let size = match v.lines {
        Some((a, r)) if !v.binary => Some((a, r)),
        _ => None,
    };
    let bar_w = 5.0 * TICK.x + 4.0 * TICK_GAP;
    let bar_x = rect.max.x - bar_w - 6.0;
    if let Some((a, r)) = size {
        let (green, red) = ticks(a, r);
        for k in 0..5u8 {
            let colour = if k < green {
                p.good
            } else if k >= 5 - red {
                p.link_missing
            } else {
                p.border
            };
            let min = Pos2::new(bar_x + f32::from(k) * (TICK.x + TICK_GAP), rect.min.y + 6.0);
            ui.painter().rect_filled(Rect::from_min_size(min, TICK), 1.5, colour);
        }
        let counts = size_text(size);
        let galley = ui.painter().layout_no_wrap(counts, FontId::monospace(11.5), p.muted);
        let at = Pos2::new(bar_x - galley.size().x - 6.0, rect.min.y + 6.5);
        ui.painter().galley(at, galley, p.muted);
    } else {
        let galley = ui.painter().layout_no_wrap("changed".into(), FontId::proportional(11.5), p.muted);
        ui.painter().galley(Pos2::new(rect.max.x - galley.size().x - 6.0, rect.min.y + 6.5), galley, p.muted);
    }
    let src_clip = Rect::from_min_max(Pos2::new(text_x, rect.min.y + 21.0), Pos2::new(rect.max.x - 4.0, rect.max.y));
    ui.painter().with_clip_rect(src_clip).text(Pos2::new(text_x, rect.min.y + 22.0), Align2::LEFT_TOP, source, FontId::proportional(12.0), p.muted);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{time} {source}")));
    resp.on_hover_text(format!("{}\n{}", full_time(v.time), size_hover(size)))
}
