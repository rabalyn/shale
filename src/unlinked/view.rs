//! The widget of the Unlinked notes tab (NAV-072, NAV-073).

use super::{Entry, Mode, Sort};
use crate::app::{muted, Action, ShaleApp};
use crate::layout::{icon_button, Icon};
use crate::theme;
use crate::vault::rel_to_string;
use eframe::egui::{self, Ui};

/// The day of a time in seconds on the clock of the machine, as `2026-11-15` (the day `modified:` is written in).
fn day(secs: i64) -> String {
    crate::note_props::format_timestamp(secs).chars().take(10).collect()
}

impl ShaleApp {
    pub fn unlinked_view(&mut self, ui: &mut Ui) {
        let Some(v) = &self.vault else { return };
        let list = self.unlinked.list(v);
        let st = &mut self.unlinked;
        st.rects.clear();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let full = ui.available_width();
            let col = (full - 32.0).clamp(200.0, 860.0);
            let pad = ((full - col) / 2.0).max(16.0);
            ui.horizontal(|ui| {
                ui.add_space(pad);
                ui.vertical(|ui| {
                    ui.set_width(col - 16.0);
                    ui.add_space(14.0);
                    ui.label(egui::RichText::new("Unlinked notes").size(28.0).color(theme::palette().text_strong));
                    ui.label(muted(format!("{} · {}", st.mode.hint(), if list.len() == 1 { "1 file".to_string() } else { format!("{} files", list.len()) })));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        for m in Mode::ALL {
                            let r = crate::clickable::pointer(ui.selectable_label(st.mode == m, m.label()));
                            st.rects.push((format!("mode:{}", m.name()), r.rect));
                            if r.clicked() {
                                st.mode = m;
                            }
                        }
                        ui.add_space(8.0);
                        let sort = icon_button(ui, Icon::Sort, 26.0, false, &format!("Sort: {}", st.sort.label()));
                        st.rects.push(("sort".into(), sort.rect));
                        egui::Popup::menu(&sort).show(|ui| {
                            ui.set_min_width(200.0);
                            for s in Sort::ALL {
                                if crate::clickable::pointer(ui.radio(st.sort == s, s.label())).clicked() {
                                    st.sort = s;
                                    ui.close();
                                }
                            }
                        });
                        ui.label(muted(st.sort.label()).size(12.0));
                    });
                    ui.add_space(10.0);
                    if list.is_empty() {
                        ui.add_space(10.0);
                        ui.label(muted(st.mode.empty()));
                    }
                    let actions = &mut self.actions;
                    st.rows.show(ui, list.len(), 24.0, |ui, i| row(ui, &list[i], actions));
                    ui.add_space(40.0);
                });
            });
        });
    }
}

fn row(ui: &mut Ui, e: &Entry, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        if crate::clickable::clickable(ui, egui::Label::new(egui::RichText::new(&e.name).size(15.0).color(theme::palette().text_strong))).clicked() {
            actions.push(Action::Open { rel: e.rel.clone(), new_tab: false });
        }
        let folder = e.rel.parent().map(rel_to_string).filter(|s| !s.is_empty());
        let mut meta = vec![];
        if e.drawing {
            meta.push("drawing".to_string());
        }
        meta.extend(folder);
        meta.extend(e.changed.map(day));
        if !meta.is_empty() {
            ui.label(muted(meta.join(" · ")).size(12.0));
        }
    });
}
