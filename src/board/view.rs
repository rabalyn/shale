//! The window of a board (BOARD-007, BOARD-010, BOARD-012, BOARD-014): the strip of boards, the
//! lanes and their cards, the lane menu and the settings popover of a lane (QUERY-028). It draws
//! what it is given and says what the user asked for as [`Act`]s; the window carries them out
//! (`board/app.rs`), so the file and the view cannot disagree (BOARD-006).

use super::card::{day_text, Card};
use super::drag::Dropped;
use super::file::{Chip, Props};
use super::lanes::drop_property;
use super::{Info, LaneView};
use crate::note_props::Priority;
use crate::reader::SceneCache;
use crate::search_all::query::Cache;
use crate::search_form::{self, Opts};
use crate::search_sources::Prefixes;
use crate::theme;
use crate::vault::Vault;
use eframe::egui::{self, Align, Color32, CornerRadius, Id, Key, LayerId, Layout, Margin, Modifiers, Order, Popup, PopupCloseBehavior, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use std::path::{Path, PathBuf};

/// The width of a lane (points).
const LANE_WIDTH: f32 = 280.0;
/// The height of the picture of a drawing on its card.
const THUMB_HEIGHT: f32 = 90.0;
/// How many tags a card shows before "+n".
const MAX_TAGS: usize = 3;

/// What the user asked for in this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// show another board
    Switch(PathBuf),
    /// make a board note with a starter lane and show it
    NewBoard,
    /// open the board's note in the editor, in a tab
    EditFile,
    /// the export dialog of the board: a PDF of the board and its notes (BOARD-030)
    ExportPdf,
    AddLane,
    RemoveLane(usize),
    RenameLane(usize, String),
    /// move lane `from` to the place `to`
    MoveLane(usize, usize),
    SetQuery(usize, String),
    SetSort(usize, String),
    SetDrop(usize, String),
    SetDone(usize, bool),
    /// `cards: every-lane` on or off
    SetEveryLane(bool),
    /// the chips of the cards
    SetShows(Vec<Chip>),
    /// select the card (arrow keys)
    Select(PathBuf),
    /// select the card and open its peek next to `Rect` (BOARD-012)
    Peek(PathBuf, Rect),
    /// open the card's note in a tab (BOARD-014)
    OpenTab(PathBuf),
    /// a card was dropped on a lane (BOARD-019 to BOARD-022)
    Drop(Dropped),
    /// the Advanced switch of the form is a setting (QUERY-026)
    SaveSettings,
}

/// Where a lane and its cards were drawn in this frame, to find what a dragged card is over.
struct Place {
    lane: Rect,
    cards: Vec<Rect>,
}

/// The lane and the place among its cards that the pointer at `pos` is over: the number of cards
/// whose middle is above it.
fn target(places: &[Place], pos: Pos2) -> Option<(usize, usize)> {
    let (i, p) = places.iter().enumerate().find(|(_, p)| p.lane.contains(pos))?;
    Some((i, p.cards.iter().filter(|r| r.center().y < pos.y).count()))
}

/// The settings popover of one lane (QUERY-028): the form edits copies of the lane's lines.
pub struct Settings {
    pub lane: usize,
    form: search_form::State,
    cache: Cache,
    query: String,
    sort: String,
    drop: String,
    /// what the lane's lines were when they were last taken over or written: a different text in the
    /// file is a hand edit, which the popover shows
    seen: (String, String, String),
}

impl Settings {
    fn new(lane: usize, v: &LaneView) -> Settings {
        let mut form = search_form::State::default();
        form.focus();
        Settings { lane, form, cache: Cache::for_lane(), query: v.query.clone(), sort: v.sort.clone(), drop: v.drop.clone(), seen: (v.query.clone(), v.sort.clone(), v.drop.clone()) }
    }

    /// The text of the file changed by hand while the popover was open.
    fn follow(&mut self, v: &LaneView) {
        let now = (v.query.clone(), v.sort.clone(), v.drop.clone());
        if now != self.seen {
            (self.query, self.sort, self.drop) = now.clone();
            self.seen = now;
        }
    }
}

/// What the view keeps between frames.
#[derive(Default)]
pub struct State {
    pub settings: Option<Settings>,
    /// the lane that is being renamed, with the text so far
    rename: Option<(usize, String)>,
    /// the name is selected when its field first shows, so that typing replaces it
    rename_fresh: bool,
    /// the selected card is scrolled into view when it is next drawn
    reveal: bool,
    /// the lane that is asked about deleting: its place and name
    confirm: Option<(usize, String)>,
    /// the popover is to be opened for this lane in this frame (a double click on the heading)
    open_settings: Option<usize>,
    /// the card that is being dragged and the lane it was taken from (BOARD-019)
    drag: Option<(PathBuf, usize)>,
    /// where the dragged card would land now: the lane and the place among its cards
    pub(crate) over: Option<(usize, usize)>,
    /// where the lanes were drawn in this frame
    places: Vec<Place>,
}

impl State {
    /// Open the settings popover of lane `i` (the ⋯ menu and a double click on the heading).
    pub fn open_settings(&mut self, i: usize) {
        self.open_settings = Some(i);
    }

    /// Start renaming lane `i`.
    pub fn rename(&mut self, i: usize, name: &str) {
        self.rename = Some((i, name.to_string()));
        self.rename_fresh = true;
    }

    /// Is a card being dragged? Esc then cancels the drag and nothing else.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// A card was selected with the keys: show it.
    pub fn reveal(&mut self) {
        self.reveal = true;
    }
}

/// What the view reads of the window.
pub struct Env<'a> {
    pub vault: &'a Vault,
    pub scenes: &'a mut SceneCache,
    pub dark: bool,
    /// the Advanced switch of the form (QUERY-026)
    pub advanced: &'a mut bool,
}

/// What the view draws.
pub struct Data<'a> {
    pub boards: &'a [Info],
    pub current: Option<&'a Path>,
    pub props: &'a Props,
    pub lanes: &'a [LaneView],
    pub selected: Option<&'a Path>,
    /// the arrow keys and Enter move and open the selection: nothing else has the keyboard
    pub keys: bool,
}

/// Draw the board view; the answer is what was asked for.
pub fn show(ui: &mut Ui, st: &mut State, data: &Data, env: &mut Env) -> Vec<Act> {
    let mut acts = Vec::new();
    strip(ui, data, &mut acts);
    ui.add_space(4.0);
    if data.current.is_none() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("No board in this vault yet").size(18.0).color(theme::palette().text));
            ui.add_space(6.0);
            theme::hint(ui, "A board is a note with `type: board` in its front matter. Its lanes are made from queries.");
            if ui.button("New board").clicked() {
                acts.push(Act::NewBoard);
            }
        });
        return acts;
    }
    if data.keys {
        keys(ui, st, data, &mut acts);
    }
    if let Some(i) = st.open_settings.take() {
        if let Some(l) = data.lanes.get(i) {
            st.settings = Some(Settings::new(i, l));
        }
    }
    let height = ui.available_height();
    st.places.clear();
    egui::ScrollArea::horizontal().id_salt("board-lanes").auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            for (i, lane) in data.lanes.iter().enumerate() {
                ui.allocate_ui_with_layout(Vec2::new(LANE_WIDTH, height - 14.0), Layout::top_down(Align::Min), |ui| lane_ui(ui, i, lane, st, data, env, &mut acts));
                ui.add_space(6.0);
            }
        });
    });
    drag_end(ui, st, data, &mut acts);
    confirm_delete(ui.ctx(), st, &mut acts);
    acts
}

/// While a card is dragged: the lane and the place it would land in are marked and a ghost of the card
/// follows the pointer. Letting go on a lane drops the card there, Esc and letting go elsewhere do
/// nothing (BOARD-019).
fn drag_end(ui: &Ui, st: &mut State, data: &Data, acts: &mut Vec<Act>) {
    let Some((rel, from)) = st.drag.clone() else {
        st.over = None;
        return;
    };
    if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        st.drag = None;
        st.over = None;
        return;
    }
    let pos = ui.input(|i| i.pointer.interact_pos());
    st.over = pos.and_then(|p| target(&st.places, p));
    let ctx = ui.ctx();
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    let pal = theme::palette();
    let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::unique("board-drag")));
    if let Some((to, index)) = st.over {
        let place = &st.places[to];
        painter.rect_stroke(place.lane, CornerRadius::same(8), Stroke::new(2.0, pal.accent), egui::StrokeKind::Inside);
        let y = match (place.cards.get(index), place.cards.last()) {
            (Some(r), _) => r.top() - 2.0,
            (None, Some(r)) => r.bottom() + 2.0,
            (None, None) => place.lane.top() + 44.0,
        };
        painter.line_segment([Pos2::new(place.lane.left() + 10.0, y), Pos2::new(place.lane.right() - 10.0, y)], Stroke::new(3.0, pal.accent));
    }
    if let (Some(p), Some(card)) = (pos, data.lanes.get(from).and_then(|l| l.cards.iter().find(|c| c.rel == rel))) {
        let galley = painter.layout_no_wrap(card.title.clone(), egui::FontId::proportional(13.0), pal.text_strong);
        let rect = Rect::from_min_size(p + Vec2::new(12.0, 8.0), galley.size() + Vec2::new(16.0, 10.0));
        painter.rect(rect, CornerRadius::same(6), pal.bg_primary.gamma_multiply(0.9), Stroke::new(1.0, pal.accent), egui::StrokeKind::Inside);
        painter.galley(rect.min + Vec2::new(8.0, 5.0), galley, pal.text_strong);
    }
    if ui.input(|i| i.pointer.any_released()) {
        if let Some((to, index)) = st.over {
            acts.push(Act::Drop(Dropped { card: rel, from, to, index }));
        }
        st.drag = None;
        st.over = None;
    }
}

/// The question before a lane is deleted: its lines go with it.
fn confirm_delete(ctx: &egui::Context, st: &mut State, acts: &mut Vec<Act>) {
    let Some((i, name)) = st.confirm.clone() else { return };
    let mut keep = true;
    let resp = crate::dialog::Dialog::new("board-delete-lane", "Delete lane?").width(400.0).show(ctx, |ui| {
        theme::paragraph(ui, format!("The lane “{name}” is taken out of the board's note, with its query and its pinned cards. The notes of the cards are not changed."));
        ui.horizontal(|ui| {
            if ui.button(RichText::new("Delete lane").color(theme::palette().link_missing)).clicked() {
                acts.push(Act::RemoveLane(i));
                keep = false;
            }
            if ui.button("Cancel").clicked() {
                keep = false;
            }
        });
    });
    if resp.should_close() || !keep {
        st.confirm = None;
    }
}

/// The boards of the vault in a row, the new-board button and the menu of the board itself
/// (BOARD-007).
fn strip(ui: &mut Ui, data: &Data, acts: &mut Vec<Act>) {
    ui.horizontal(|ui| {
        egui::ScrollArea::horizontal().id_salt("board-strip").max_width((ui.available_width() - 190.0).max(100.0)).show(ui, |ui| {
            ui.horizontal(|ui| {
                for b in data.boards {
                    let here = data.current == Some(b.rel.as_path());
                    if ui.selectable_label(here, &b.title).on_hover_text(b.rel.to_string_lossy()).clicked() && !here {
                        acts.push(Act::Switch(b.rel.clone()));
                    }
                }
            });
        });
        if ui.button("+ New board").on_hover_text("A note with type: board and one lane").clicked() {
            acts.push(Act::NewBoard);
        }
        if data.current.is_some() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                ui.menu_button("Board", |ui| {
                    let mut every = data.props.every_lane;
                    if ui.checkbox(&mut every, "A card in every lane it matches").on_hover_text("cards: every-lane. Off: a card is in the first lane that matches it").changed() {
                        acts.push(Act::SetEveryLane(every));
                    }
                    ui.separator();
                    ui.label(RichText::new("Cards show").color(theme::palette().muted));
                    for chip in Chip::ALL {
                        let mut on = data.props.shows.contains(&chip);
                        if ui.checkbox(&mut on, chip.word()).changed() {
                            let mut shows = data.props.shows.clone();
                            shows.retain(|c| *c != chip);
                            if on {
                                shows.push(chip);
                            }
                            shows.sort_by_key(|c| Chip::ALL.iter().position(|a| a == c));
                            acts.push(Act::SetShows(shows));
                        }
                    }
                });
                if ui.button("+ Add lane").on_hover_text("A lane at the end of the board; set its query with its ⋯ menu").clicked() {
                    acts.push(Act::AddLane);
                }
                if ui.button("Edit file").on_hover_text("Open the note of the board in a tab").clicked() {
                    acts.push(Act::EditFile);
                }
                if ui.button("Export PDF…").on_hover_text("The board on landscape pages, then the notes of its cards").clicked() {
                    acts.push(Act::ExportPdf);
                }
            });
        }
    });
}

/// The arrow keys move the selection between the cards, Enter opens the selected one (BOARD-014).
fn keys(ui: &Ui, st: &mut State, data: &Data, acts: &mut Vec<Act>) {
    let (down, up, left, right, enter) = ui.input(|i| (i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowLeft), i.key_pressed(Key::ArrowRight), i.key_pressed(Key::Enter)));
    let at = data.selected.and_then(|s| data.lanes.iter().enumerate().find_map(|(l, lane)| lane.cards.iter().position(|c| c.rel == s).map(|c| (l, c))));
    if enter {
        if let Some(rel) = data.selected {
            acts.push(Act::OpenTab(rel.to_path_buf()));
        }
        return;
    }
    let target = match at {
        None if down || up || left || right => data.lanes.iter().enumerate().find(|(_, l)| !l.cards.is_empty()).map(|(l, _)| (l, 0)),
        Some((l, c)) if down => Some((l, (c + 1).min(data.lanes[l].cards.len() - 1))),
        Some((l, c)) if up => Some((l, c.saturating_sub(1))),
        Some((l, c)) if left || right => {
            // the next lane in that direction that has cards, at the same height or the last
            let order: Vec<usize> = if left { (0..l).rev().collect() } else { (l + 1..data.lanes.len()).collect() };
            order.into_iter().find(|n| !data.lanes[*n].cards.is_empty()).map(|n| (n, c.min(data.lanes[n].cards.len() - 1)))
        }
        _ => None,
    };
    if let Some((l, c)) = target {
        let rel = data.lanes[l].cards[c].rel.clone();
        if data.selected != Some(rel.as_path()) {
            st.reveal();
            acts.push(Act::Select(rel));
        }
    }
}

fn lane_ui(ui: &mut Ui, i: usize, lane: &LaneView, st: &mut State, data: &Data, env: &mut Env, acts: &mut Vec<Act>) {
    let mut cards = Vec::new();
    let out = egui::Frame::new().fill(theme::palette().bg_secondary).stroke(Stroke::new(1.0, theme::palette().border)).corner_radius(CornerRadius::same(8)).inner_margin(Margin::same(8)).show(ui, |ui| {
        ui.set_width(LANE_WIDTH - 18.0);
        header(ui, i, lane, st, data, env, acts);
        if let Some(e) = &lane.error {
            theme::problem(ui, e.clone());
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical().id_salt(("board-lane", i)).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if lane.waiting && lane.cards.is_empty() {
                theme::hint(ui, "Searching…");
            } else if lane.cards.is_empty() && lane.error.is_none() {
                theme::hint(ui, "No cards");
            }
            for card in &lane.cards {
                cards.push(card_ui(ui, i, card, data, st, env, acts));
                ui.add_space(4.0);
            }
            if lane.partial {
                theme::hint(ui, "The search stopped early: the list is not complete");
            }
        });
    });
    st.places.push(Place { lane: out.response.rect, cards });
}

/// The heading of a lane: its name (double-click for the settings), the number of cards and the ⋯ menu.
fn header(ui: &mut Ui, i: usize, lane: &LaneView, st: &mut State, data: &Data, env: &mut Env, acts: &mut Vec<Act>) {
    ui.horizontal(|ui| {
        if st.rename.as_ref().is_some_and(|(n, _)| *n == i) {
            let (_, text) = st.rename.as_mut().expect("checked");
            let id = Id::unique(("lane-rename", i));
            let r = ui.add(egui::TextEdit::singleline(text).desired_width(LANE_WIDTH - 90.0).id(id));
            r.request_focus();
            if std::mem::take(&mut st.rename_fresh) {
                let mut edit = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
                edit.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()))));
                edit.store(ui.ctx(), id);
            }
            let (enter, esc) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if enter {
                let (_, text) = st.rename.take().expect("checked");
                acts.push(Act::RenameLane(i, text));
            } else if esc || r.lost_focus() {
                st.rename = None;
            }
        } else {
            let name = ui.add(egui::Label::new(RichText::new(&lane.name).strong().size(15.0).color(theme::palette().text_strong)).sense(Sense::click()).truncate().selectable(false));
            if name.double_clicked() {
                st.open_settings(i);
            }
            name.on_hover_text("Double-click for the settings of the lane");
            ui.label(RichText::new(lane.cards.len().to_string()).color(theme::palette().muted));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let more = ui.menu_button("⋯", |ui| {
                if ui.button("Settings…").clicked() {
                    st.open_settings(i);
                    ui.close();
                }
                if ui.button("Rename").clicked() {
                    st.rename(i, &lane.name);
                    ui.close();
                }
                if ui.add_enabled(i > 0, egui::Button::new("Move left")).clicked() {
                    acts.push(Act::MoveLane(i, i - 1));
                    ui.close();
                }
                if ui.add_enabled(i + 1 < data.lanes.len(), egui::Button::new("Move right")).clicked() {
                    acts.push(Act::MoveLane(i, i + 1));
                    ui.close();
                }
                if ui.button("Delete lane…").on_hover_text("Takes the lane, its query and its pinned cards out of the file").clicked() {
                    st.confirm = Some((i, lane.name.clone()));
                    ui.close();
                }
            });
            if st.settings.as_ref().is_some_and(|s| s.lane == i) {
                settings(&more.response, st, data, i, env, acts);
            }
        });
    });
}

/// The settings popover of lane `i` (QUERY-028): the form of the filter and the sort, the `drop:`
/// property and the `done:` switch. What the form changes is written to the file at once.
fn settings(anchor: &egui::Response, st: &mut State, data: &Data, i: usize, env: &mut Env, acts: &mut Vec<Act>) {
    let Some(lane) = data.lanes.get(i) else {
        st.settings = None;
        return;
    };
    let mut open = true;
    let Some(s) = st.settings.as_mut() else { return };
    s.follow(lane);
    let mut asked = Vec::new();
    Popup::from_response(anchor).id(Id::unique(("board-lane-settings", i))).open_bool(&mut open).close_behavior(PopupCloseBehavior::CloseOnClickOutside).width(380.0).show(|ui| {
        ui.label(RichText::new(format!("Lane “{}”", lane.name)).strong());
        ui.add_space(4.0);
        let count = search_form::matches_text(lane.cards.len(), lane.partial);
        let opts = Opts { prefixes: Prefixes::default(), host_box: false, count: Some(&count) };
        let changed = search_form::show(ui, &mut s.form, &mut s.query, &mut s.cache, env.advanced, Some(&mut s.sort), &opts);
        if changed.query {
            asked.push(Act::SetQuery(i, s.query.clone()));
        }
        if changed.sort {
            asked.push(Act::SetSort(i, s.sort.clone()));
        }
        if changed.advanced {
            asked.push(Act::SaveSettings);
        }
        ui.add_space(4.0);
        ui.label(RichText::new("When a card is dropped here, set").color(theme::palette().muted));
        let derived = drop_property(&s.query);
        let hint = derived.clone().unwrap_or_else(|| "a property such as status: doing".to_string());
        let r = ui.add(egui::TextEdit::singleline(&mut s.drop).hint_text(hint).desired_width(f32::INFINITY));
        if r.changed() {
            asked.push(Act::SetDrop(i, s.drop.clone()));
        }
        let mut done = lane.done;
        if ui.checkbox(&mut done, "Cards here are done").changed() {
            asked.push(Act::SetDone(i, done));
        }
    });
    if !asked.is_empty() {
        // what was written is what the file will say next frame
        s.seen = (s.query.clone(), s.sort.clone(), s.drop.clone());
        acts.extend(asked);
    }
    if !open {
        st.settings = None;
    }
}

/// A chip: a small rounded label that is never broken up; a row of them wraps between chips.
fn chip(ui: &mut Ui, text: &str, fg: Color32, bg: Color32, tip: &str) {
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(11.0), fg);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + Vec2::new(12.0, 2.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(8), bg);
    ui.painter().galley(rect.min + Vec2::new(6.0, 1.0), galley, fg);
    resp.on_hover_text(tip);
}

fn priority_colour(p: Priority) -> (Color32, Color32) {
    let pal = theme::palette();
    match p {
        Priority::Highest | Priority::High => (pal.warn, pal.warn_bg),
        Priority::Medium => (pal.text, pal.widget_bg),
        Priority::Low | Priority::Lowest => (pal.muted, pal.widget_bg),
    }
}

fn chips(ui: &mut Ui, card: &Card, props: &Props) {
    let pal = theme::palette();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 3.0);
        for what in &props.shows {
            match what {
                Chip::Due => {
                    if let Some(day) = card.due {
                        let (fg, bg) = if card.overdue() { (pal.link_missing, pal.bad_bg) } else { (pal.text, pal.widget_bg) };
                        chip(ui, &day_text(day), fg, bg, if card.overdue() { "Due, overdue" } else { "Due" });
                    }
                }
                Chip::Status => {
                    if let Some(s) = &card.status {
                        chip(ui, s, pal.text, pal.widget_bg, "Status");
                    }
                }
                Chip::Priority => {
                    if let Some(p) = card.priority {
                        let (fg, bg) = priority_colour(p);
                        chip(ui, p.name(), fg, bg, "Priority");
                    }
                }
                Chip::Tags => {
                    for t in card.tags.iter().take(MAX_TAGS) {
                        chip(ui, &format!("#{t}"), pal.tag, pal.tag_bg, "Tag");
                    }
                    if card.tags.len() > MAX_TAGS {
                        chip(ui, &format!("+{}", card.tags.len() - MAX_TAGS), pal.muted, pal.widget_bg, &card.tags[MAX_TAGS..].join(", "));
                    }
                }
            }
        }
    });
}

/// One card (BOARD-010): the title, the chips, the picture of a drawing. A click selects it and
/// opens the peek, Ctrl+Click opens the note in a tab (BOARD-012, BOARD-014), a drag takes it to a
/// lane (BOARD-019). Gives where it was drawn.
fn card_ui(ui: &mut Ui, lane: usize, card: &Card, data: &Data, st: &mut State, env: &mut Env, acts: &mut Vec<Act>) -> Rect {
    let pal = theme::palette();
    let selected = data.selected == Some(card.rel.as_path());
    let stroke = if selected { Stroke::new(2.0, pal.accent) } else { Stroke::new(1.0, pal.border) };
    let out = egui::Frame::new().fill(pal.bg_primary).stroke(stroke).corner_radius(CornerRadius::same(6)).inner_margin(Margin::same(8)).show(ui, |ui| {
        ui.style_mut().interaction.selectable_labels = false;
        ui.set_width(ui.available_width());
        ui.add(egui::Label::new(RichText::new(&card.title).strong().color(pal.text_strong)).wrap());
        if card.drawing {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), THUMB_HEIGHT), Sense::hover());
            let bg = if env.dark { Color32::from_rgb(18, 18, 18) } else { Color32::WHITE };
            ui.painter().rect_filled(rect, CornerRadius::same(4), bg);
            if let Some(scene) = env.scenes.get(env.vault, &card.rel) {
                let pics = crate::drawing::pictures::textures(ui.ctx(), scene, env.vault, &card.rel);
                crate::drawing::paint_thumbnail(ui.painter(), scene, rect.shrink(4.0), env.dark, &pics);
            }
        }
        chips(ui, card, data.props);
    });
    let rect = out.response.rect;
    let resp = ui.interact(rect, ui.scope_id().with(("board-card", &card.rel)), Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::PointingHand);
    if selected && st.reveal {
        st.reveal = false;
        ui.scroll_to_rect(rect, None);
    }
    if resp.drag_started() {
        st.drag = Some((card.rel.clone(), lane));
    }
    if resp.clicked() {
        if ui.input(|i| i.modifiers.ctrl || i.modifiers.command) {
            acts.push(Act::OpenTab(card.rel.clone()));
        } else {
            acts.push(Act::Peek(card.rel.clone(), rect));
        }
    }
    rect
}
