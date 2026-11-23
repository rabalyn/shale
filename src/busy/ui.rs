//! What a running task looks like (JOB-004…JOB-007): a spinner with its text and, where the work can
//! stop, Cancel; a button that is off while its action runs; and the indicator in the top right
//! of the editor pane that lists everything running in the background.

use super::{Busy, Key, TaskInfo};
use crate::theme;
use eframe::egui::{self, Align2, Area, Frame, Id, Margin, Order, Pos2, Response, Sense, Ui, Vec2, Widget};
use std::time::Duration;

/// How far the indicator sits from the right and the top edge of the pane: in the row of the tabs, clear of the content.
const INDENT: Vec2 = Vec2::new(-12.0, 5.0);

/// The widget, off while any task of `keys` runs; hovering it then says what runs. Every button
/// and key that starts an action goes through this or through `Busy::begin` (JOB-005).
pub fn button(ui: &mut Ui, busy: &Busy, keys: &[&Key], widget: impl Widget) -> Response {
    let running = keys.iter().find_map(|k| busy.info(k));
    ui.add_enabled(running.is_none(), widget).on_disabled_hover_text(running.map(|t| format!("Not now: {}", t.label.trim_end_matches('…').to_lowercase() + " is still running")).unwrap_or_default())
}

/// While the action of `key` runs: a spinner, what is happening and, if the work can stop, Cancel.
/// Returns whether it runs.
pub fn status(ui: &mut Ui, busy: &Busy, key: &Key) -> bool {
    let Some(task) = busy.info(key) else { return false };
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(14.0));
        theme::hint(ui, &task.label);
        cancel_button(ui, busy, &task);
    });
    true
}

fn cancel_button(ui: &mut Ui, busy: &Busy, task: &TaskInfo) {
    if !task.cancellable {
        return;
    }
    let stopping = task.cancelling;
    if ui.add_enabled(!stopping, egui::Button::new(if stopping { "Stopping…" } else { "Cancel" }).small()).clicked() {
        busy.cancel(task.id);
    }
}

/// "12 s", "3 min": how long a task runs.
pub fn elapsed(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{s} s")
    } else {
        format!("{} min", s / 60)
    }
}

/// What the collapsed indicator says: the text of the one task, or how many run.
pub fn summary(tasks: &[TaskInfo]) -> String {
    match tasks {
        [one] => one.label.clone(),
        many => format!("{} tasks running", many.len()),
    }
}

/// The indicator of work in the background, drawn with its top right corner at `top_right` (the
/// top right of the editor pane) plus a small indent: a spinner and the text of the task, or the
/// number of tasks. Hovering it, or a click that pins it open, lists the tasks with how long they
/// run and Cancel where the work can stop. Nothing is drawn while nothing runs (JOB-007).
pub fn indicator(ctx: &egui::Context, busy: &Busy, top_right: Pos2) {
    let now = std::time::Instant::now();
    let tasks = busy.list_at(now);
    // a task that waits to be listed (JOB-017) brings the indicator when its time has come
    if let Some(wait) = busy.next_listing(now) {
        ctx.request_repaint_after(wait);
    }
    if tasks.is_empty() {
        return;
    }
    let id = Id::unique("busy-indicator");
    let pinned = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let expanded = pinned || ctx.data(|d| d.get_temp::<bool>(id.with("hover"))).unwrap_or(false);
    let pal = theme::palette();
    let shown = Area::new(id).order(Order::Foreground).sense(Sense::click()).pivot(Align2::RIGHT_TOP).fixed_pos(top_right + INDENT).show(ctx, |ui| {
        Frame::new().fill(pal.bg_secondary).stroke(egui::Stroke::new(1.0, pal.border)).corner_radius(8.0).inner_margin(Margin::symmetric(10, 6)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            // a click on the text belongs to the indicator (it pins the list), not to a text selection
            ui.style_mut().interaction.selectable_labels = false;
            if expanded {
                egui::Grid::new(id.with("list")).num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
                    for t in &tasks {
                        ui.add(egui::Spinner::new().size(14.0));
                        ui.label(&t.label);
                        theme::hint(ui, elapsed(t.started.elapsed()));
                        cancel_button(ui, busy, t);
                        ui.end_row();
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0));
                    theme::hint(ui, summary(&tasks));
                });
            }
        })
    });
    let area = shown.response;
    let (hovered, clicked) = (area.contains_pointer(), area.clicked());
    ctx.data_mut(|d| {
        d.insert_temp(id.with("hover"), hovered);
        if clicked {
            d.insert_temp(id, !pinned);
        }
    });
    // the elapsed time and the spinner move
    ctx.request_repaint_after(Duration::from_millis(250));
}
