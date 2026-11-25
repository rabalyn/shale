//! The peek of a card (BOARD-012, BOARD-013): the floating frame of the link preview (EDIT-033,
//! the size of EDIT-037, dragged by its title and resized at its edges) with a real editor of the
//! card's note in it. The note is edited in its buffer like a note in a tab, so it saves as the
//! editor does (a checked save, DATA-003); the priority control writes the front matter only.

use super::links::{self, Edit, Lists, Picker};
use crate::app::NoteDoc;
use crate::clickable::ComboBoxPointer;
use crate::editor::{EditorAction, Note, NoteEditor};
use crate::fonts::EditorFonts;
use crate::link_preview::{clamp_size, Grip};
use crate::note_props::Priority;
use crate::search_all::SearchWorker;
use crate::theme;
use crate::undo::Histories;
use crate::vault::Vault;
use eframe::egui::{self, Align, Align2, Id, Layout, Pos2, Rect, RichText, Sense, Stroke, Ui, UiBuilder, Vec2};
use std::path::PathBuf;

/// The height of the title row of the frame.
const TITLE_HEIGHT: f32 = 26.0;

pub struct Peek {
    pub rel: PathBuf,
    pub editor: NoteEditor,
    id: Id,
    pub size: Vec2,
    /// the top left of the frame: next to the card when the peek opens, moved by dragging the title
    pos: Option<Pos2>,
    /// the card the peek belongs to, on the screen when it was opened
    anchor: Rect,
    /// where the frame was drawn last frame
    pub rect: Option<Rect>,
    /// the field under "Blocked by" or "Blocks" while one is open (BOARD-044)
    pub picker: Option<Picker>,
}

/// What the frame shows of the window.
pub struct With<'a> {
    /// the buffer of the note
    pub doc: &'a mut NoteDoc,
    pub vault: &'a Vault,
    pub undo: &'a mut Histories,
    pub fonts: &'a EditorFonts,
    pub ctrl_to_follow: bool,
    pub title: &'a str,
    pub priority: Option<Priority>,
    /// what the card waits for and what waits for it (BOARD-044)
    pub lists: Lists<'a>,
    pub worker: &'a mut SearchWorker,
}

/// What the user did in the peek in this frame.
#[derive(Default)]
pub struct Out {
    /// the text of the note changed
    pub changed: bool,
    /// the × button
    pub close: bool,
    /// the "Open" button: the note in a tab (BOARD-014)
    pub open_tab: bool,
    /// a new priority was chosen (BOARD-013); none is "no priority"
    pub priority: Option<Option<Priority>>,
    /// a link or the like was followed in the editor
    pub action: Option<EditorAction>,
    /// the frame was resized: the size to keep as the setting of EDIT-037
    pub resized: Option<Vec2>,
    /// a blocker was added or taken away (BOARD-044)
    pub edit: Option<Edit>,
}

impl Peek {
    pub fn new(rel: PathBuf, anchor: Rect, size: Vec2) -> Peek {
        let (editor, id) = (NoteEditor::new(Id::unique(("board-peek-editor", &rel))), Id::unique(("board-peek", &rel)));
        Peek { rel, editor, id, size, pos: None, anchor, rect: None, picker: None }
    }

    /// Where the frame goes when it opens, at the height of the card: right of it, else left of it,
    /// else at the right edge of the window (over the edge of the card, not over the lanes before it).
    fn placed(&self, screen: Rect) -> Pos2 {
        let (w, gap) = (self.size.x, 8.0);
        let (right, left) = (self.anchor.max.x + gap, self.anchor.min.x - gap - w);
        let x = if right + w <= screen.right() - 8.0 {
            right
        } else if left >= screen.left() + 8.0 {
            left
        } else {
            screen.right() - w - 8.0
        };
        Pos2::new(x, self.anchor.min.y)
    }

    /// Draw the frame over the window.
    pub fn show(&mut self, ctx: &egui::Context, note: With) -> Out {
        let With { doc, vault, undo, fonts, ctrl_to_follow, title, priority, lists, worker } = note;
        let mut out = Out::default();
        let screen = ctx.content_rect();
        self.size = clamp_size(self.size, screen.size());
        let size = self.size;
        let placed = self.pos.unwrap_or_else(|| self.placed(screen));
        let pos = Pos2::new(placed.x.clamp(screen.left() + 8.0, (screen.right() - size.x - 8.0).max(screen.left() + 8.0)), placed.y.clamp(screen.top() + 8.0, (screen.bottom() - size.y - 8.0).max(screen.top() + 8.0)));
        self.pos = Some(pos);
        let stroke = Stroke::new(2.0, theme::palette().accent);
        let id = self.id;
        let (mut moved, mut grip_drag, mut ended) = (Vec2::ZERO, None, false);
        let area = egui::Area::new(id).order(egui::Order::Foreground).fixed_pos(pos).pivot(Align2::LEFT_TOP).constrain(true).show(ctx, |ui| {
            let frame = egui::Frame::popup(ui.style()).stroke(stroke);
            let pad = frame.total_margin().sum();
            let shown = frame.show(ui, |ui| {
                ui.set_width(size.x - pad.x);
                ui.set_height(size.y - pad.y);
                moved = title_row(ui, id, &mut out, title, priority);
                ui.add_space(4.0);
                out.edit = links::show(ui, &mut self.picker, worker, vault, &lists);
                ui.add_space(4.0);
                let height = ui.available_height();
                egui::ScrollArea::vertical().id_salt((id, "scroll")).max_height(height).auto_shrink([false, false]).show(ui, |ui| match &doc.unreadable {
                    Some(why) => {
                        ui.label(RichText::new(format!("This note could not be read ({why}). It is shown read-only, and Shale will not change the file.")).color(theme::palette().link_missing));
                    }
                    None => {
                        let (changed, action) = crate::fonts::with_family(&fonts.rendered, || self.editor.show(ui, &mut doc.text, vault, Note { rel: &self.rel, undo }, fonts, ctrl_to_follow));
                        out.changed = changed;
                        out.action = action;
                    }
                });
            });
            // the handles of the edges, as the link preview has them (EDIT-037)
            for grip in Grip::ALL {
                let resp = ui.interact(grip.rect(shown.response.rect), id.with(("grip", grip.x, grip.y)), Sense::drag()).on_hover_cursor(grip.cursor());
                if resp.dragged() {
                    ui.ctx().set_cursor_icon(grip.cursor());
                    grip_drag = Some((grip, resp.drag_delta()));
                }
                ended |= resp.drag_stopped();
            }
        });
        self.rect = Some(area.response.rect);
        if moved != Vec2::ZERO {
            self.pos = Some(pos + moved);
        }
        if let Some((grip, delta)) = grip_drag {
            let new = clamp_size(size + Vec2::new(delta.x * f32::from(grip.x), delta.y * f32::from(grip.y)), screen.size());
            // the edges at the left and the top move the frame with them
            let shift = Vec2::new(if grip.x < 0 { size.x - new.x } else { 0.0 }, if grip.y < 0 { size.y - new.y } else { 0.0 });
            self.size = new;
            self.pos = Some(pos + shift);
            ctx.request_repaint();
        }
        if ended {
            out.resized = Some(self.size);
        }
        out
    }
}

/// The title of the note, the priority, "Open" and "×"; the answer is how far the title was dragged.
fn title_row(ui: &mut Ui, id: Id, out: &mut Out, title: &str, priority: Option<Priority>) -> Vec2 {
    let row = Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), TITLE_HEIGHT));
    // (the drag area is made first, so that the buttons drawn after it are above it)
    let drag = ui.interact(row, id.with("title"), Sense::drag()).on_hover_cursor(egui::CursorIcon::Grab);
    ui.scope_builder(UiBuilder::new().max_rect(row), |ui| {
        ui.horizontal_centered(|ui| {
            // (not selectable: a label that can be selected takes the drag of the title)
            ui.add(egui::Label::new(RichText::new(title).strong().color(theme::palette().text_strong)).truncate().selectable(false));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("×").on_hover_text("Close (Esc)").clicked() {
                    out.close = true;
                }
                if ui.button("Open").on_hover_text("The note in a tab (Enter on the card)").clicked() {
                    out.open_tab = true;
                }
                let mut chosen = priority;
                egui::ComboBox::from_id_salt((id, "priority")).selected_text(format!("Priority: {}", priority.map_or("none", Priority::name))).width(130.0).show_pointer(ui, |ui| {
                    crate::clickable::pointer(ui.selectable_value(&mut chosen, None, "none"));
                    for p in Priority::ALL {
                        crate::clickable::pointer(ui.selectable_value(&mut chosen, Some(p), p.name()));
                    }
                });
                if chosen != priority {
                    out.priority = Some(chosen);
                }
            });
        });
    });
    ui.advance_cursor_after_rect(row);
    if drag.dragged() {
        drag.drag_delta()
    } else {
        Vec2::ZERO
    }
}
