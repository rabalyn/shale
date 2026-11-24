//! The panes of the window: the middle area split side by side, each pane with its own tab bar
//! (NAV-032).
//!
//! `ShaleApp::tabbar` always holds the tab bar of the pane that is being worked on (the focused
//! one, or while the panes are drawn the one being drawn), so everything that means "the current
//! tab" goes through it unchanged. The other panes keep their tab bars here; the slot of the
//! pane whose bar is in `ShaleApp::tabbar` holds an empty placeholder.

use super::tabs::TabBar;
use super::{rel_to_string, ShaleApp, Tab, View};
use crate::vault_state::{SavedCalendar, SavedGraph, SavedPane, SavedTab, SavedTasks};
use eframe::egui::Rect;

/// No pane is drawn narrower than this many points (NAV-034).
pub const MIN_PANE_W: f32 = 240.0;
/// The width of the draggable border between two panes.
pub const BORDER_W: f32 = 5.0;

pub struct Panes {
    bars: Vec<TabBar>,
    /// the pane whose tab bar is in `ShaleApp::tabbar` right now
    cur: usize,
    /// the pane that has the focus: the one the global tab actions work on
    pub focused: usize,
    /// the share of the width of every pane (any positive numbers; only the ratios count)
    pub weights: Vec<f32>,
    /// where each pane was drawn last frame, and the borders between them (for tests)
    pub(crate) rects: Vec<Rect>,
    pub(crate) borders: Vec<Rect>,
    /// did a widget have the keyboard at the start of the last frame (the keyboard guard, NAV-037)
    had_keyboard: bool,
    /// was the pointer pressed or held at the start of the last frame (NAV-037)
    pressed_last: bool,
}

impl Panes {
    pub fn new() -> Self {
        Panes { bars: vec![TabBar::new()], cur: 0, focused: 0, weights: vec![1.0], rects: vec![], borders: vec![], had_keyboard: false, pressed_last: false }
    }

    pub fn len(&self) -> usize {
        self.bars.len()
    }

    /// The pane that is being drawn (the one whose tab bar is in `ShaleApp::tabbar`).
    pub(crate) fn drawn(&self) -> usize {
        self.cur
    }

    /// Back to one pane (the tab bar in `ShaleApp::tabbar` stays).
    pub(crate) fn reset(&mut self) {
        *self = Panes::new();
    }
}

/// The widths of panes with these weights in `total` points: in proportion to the weights, none
/// below `min` (the others give up the room in proportion to what they have above `min`); when
/// there is no room for the minimum, all are equal.
pub fn pane_widths(weights: &[f32], total: f32, min: f32) -> Vec<f32> {
    let n = weights.len();
    if n == 0 {
        return vec![];
    }
    let sum: f32 = weights.iter().map(|w| w.max(0.0)).sum();
    if sum <= 0.0 || total < n as f32 * min {
        return vec![total.max(0.0) / n as f32; n];
    }
    let mut widths: Vec<f32> = weights.iter().map(|w| (w.max(0.0) / sum * total).max(min)).collect();
    let excess = widths.iter().sum::<f32>() - total;
    if excess > 0.0 {
        let slack: f32 = widths.iter().map(|w| w - min).sum();
        if slack > 0.0 {
            for w in &mut widths {
                *w -= (*w - min) / slack * excess;
            }
        }
    }
    widths
}

/// The weights after the border in front of pane `i` (between `i - 1` and `i`) was moved by
/// `delta` points in `total` points: the two neighbours trade width, neither goes below `min`,
/// the others stay as they are.
pub fn drag_weights(weights: &[f32], i: usize, delta: f32, total: f32, min: f32) -> Vec<f32> {
    let mut widths = pane_widths(weights, total, min);
    if i == 0 || i >= widths.len() || total <= 0.0 {
        return weights.to_vec();
    }
    let pair = widths[i - 1] + widths[i];
    if pair < 2.0 * min {
        return weights.to_vec();
    }
    widths[i - 1] = (widths[i - 1] + delta).clamp(min, pair - min);
    widths[i] = pair - widths[i - 1];
    widths.iter().map(|w| w / total).collect()
}

impl ShaleApp {
    /// Make the tab bar of pane `i` the one in `self.tabbar`.
    pub(crate) fn enter_pane(&mut self, i: usize) {
        let i = i.min(self.panes.bars.len() - 1);
        if i == self.panes.cur {
            return;
        }
        let cur = self.panes.cur;
        std::mem::swap(&mut self.tabbar, &mut self.panes.bars[cur]);
        std::mem::swap(&mut self.tabbar, &mut self.panes.bars[i]);
        self.panes.cur = i;
        // tab ids stay unique across the panes: a bar goes on counting after the highest
        let next = self.panes.bars.iter().map(|b| b.next_tab_id).max().unwrap_or(1).max(self.tabbar.next_tab_id);
        self.tabbar.next_tab_id = next;
    }

    /// Give pane `i` the focus: the global tab actions work on it from now on.
    pub fn focus_pane(&mut self, i: usize) {
        let i = i.min(self.panes.bars.len() - 1);
        self.enter_pane(i);
        self.panes.focused = i;
    }

    /// The tab bar of pane `i`.
    pub fn pane_bar(&self, i: usize) -> &TabBar {
        if i == self.panes.cur {
            &self.tabbar
        } else {
            &self.panes.bars[i]
        }
    }

    /// The tabs of all panes, left to right.
    pub fn all_tabs(&self) -> impl Iterator<Item = &Tab> {
        (0..self.panes.len()).flat_map(|i| self.pane_bar(i).tabs.iter())
    }

    /// The tabs of all panes, to change.
    pub fn all_tabs_mut(&mut self) -> impl Iterator<Item = &mut Tab> {
        let cur = self.panes.cur;
        std::iter::once(&mut self.tabbar).chain(self.panes.bars.iter_mut().enumerate().filter(move |(i, _)| *i != cur).map(|(_, b)| b)).flat_map(|b| b.tabs.iter_mut())
    }

    /// The tab that is open in the focused pane.
    pub fn current_tab(&self) -> Option<&Tab> {
        let bar = self.pane_bar(self.panes.focused);
        bar.tabs.get(bar.active)
    }

    /// Add an empty pane to the right of the focused one, taking half of its width, and focus it.
    pub(crate) fn add_pane_right(&mut self) {
        self.focus_pane(self.panes.focused);
        let at = self.panes.focused + 1;
        let mut bar = TabBar::new();
        bar.next_tab_id = self.tabbar.next_tab_id;
        self.panes.bars.insert(at, bar);
        let half = self.panes.weights[at - 1] / 2.0;
        self.panes.weights[at - 1] = half;
        self.panes.weights.insert(at, half);
        self.focus_pane(at);
    }

    /// Open `view` in a new tab of a new pane right of the focused one and focus it (NAV-033).
    pub fn open_in_split_right(&mut self, view: View) {
        self.in_split_right(|app| match view {
            View::Note(rel) | View::Drawing(rel) => app.open(&rel, true),
            other => app.show_view(other, true),
        });
    }

    /// Run `open` with a new pane right of the focused one as the focused pane; it opens what the
    /// pane shows (a view, a link of the link preview, EDIT-036).
    pub(crate) fn in_split_right(&mut self, open: impl FnOnce(&mut Self)) {
        self.add_pane_right();
        open(self);
        // a file that could not be opened leaves no empty pane behind
        if self.tabbar.tabs.is_empty() {
            self.close_pane(self.panes.focused);
        }
        self.remember_open_files();
    }

    /// Split: the tab that is open in the focused pane also opens in a new pane to its right.
    pub fn split_right(&mut self) {
        let view = self.current_tab().map(|t| t.view.clone()).unwrap_or(View::Empty);
        self.open_in_split_right(view);
    }

    /// Open tab `i` of the focused pane in a new pane to the right (the tab menu).
    pub fn open_tab_in_split_right(&mut self, i: usize) {
        if let Some(view) = self.tabbar.tabs.get(i).map(|t| t.view.clone()) {
            self.open_in_split_right(view);
        }
    }

    /// Focus the next (1) or previous (-1) pane, wrapping around.
    pub fn step_pane(&mut self, delta: isize) {
        let n = self.panes.len() as isize;
        if n > 1 {
            self.focus_pane((self.panes.focused as isize + delta).rem_euclid(n) as usize);
        }
    }

    /// Close pane `i` with all its tabs; its width goes to its left neighbour (the first pane:
    /// the right one) and so does the focus when the pane had it. The last pane stays.
    pub fn close_pane(&mut self, i: usize) {
        let n = self.panes.len();
        if n <= 1 || i >= n {
            return;
        }
        self.flush_all();
        self.undo.flush_all();
        let focused = self.panes.focused;
        let neighbour = if i > 0 { i - 1 } else { 1 };
        // the bar to drop is in its slot, and `self.tabbar` is the bar of a pane that stays
        self.focus_pane(if i == focused { neighbour } else { focused });
        let gone: Vec<View> = self.panes.bars[i].tabs.iter().map(|t| t.view.clone()).collect();
        let (keep_focus, keep_cur) = (self.panes.focused, self.panes.cur);
        self.panes.bars.remove(i);
        let weight = self.panes.weights.remove(i);
        self.panes.weights[if i > 0 { i - 1 } else { 0 }] += weight;
        let shift = |k: usize| if k > i { k - 1 } else { k };
        self.panes.cur = shift(keep_cur);
        self.panes.focused = shift(keep_focus);
        // the sound of a note stops with its last tab (MEDIA-007)
        for view in gone {
            if let View::Note(rel) = &view {
                if !self.all_tabs().any(|t| t.view == view) {
                    self.media.stop_for_note(rel);
                }
            }
        }
        self.remember_open_files();
        self.keyboard_after_close();
    }

    /// After a tab or pane was closed the keyboard goes into the editor of the open tab of the
    /// focused pane, so it is not left nowhere (NAV-037). Closing is done from the middle area;
    /// when the sidebar is being browsed or the search box has the keyboard, it stays there.
    pub(crate) fn keyboard_after_close(&mut self) {
        if self.tree.sel.is_none() && self.search.sel.is_none() && !self.search.focus {
            self.focus_editor();
        }
    }

    /// The guard of NAV-037, called at the start of every frame: when a widget had the keyboard
    /// at the start of the last frame, none has it now (its tab, pane, popup or dialog is gone),
    /// the window has the focus, nothing was pressed with the pointer this frame (a click on
    /// empty space is the user's own choice), no dialog or popup is open, the sidebar is not browsed, the
    /// search box does not want the keyboard and the open tab is not the mail view (it has its
    /// own panes keyboard) and has no table cell being edited, the keyboard goes into the editor of the focused pane's open tab.
    pub(crate) fn keyboard_guard(&mut self, ctx: &eframe::egui::Context) {
        let has = ctx.memory(|m| m.focused()).is_some();
        let lost = self.panes.had_keyboard && !has;
        self.panes.had_keyboard = has;
        // a press drops the focus while the frame runs, so the loss shows in the next frame, when
        // the press is over: that frame counts as pressed too
        let pressed = ctx.input(|i| i.pointer.any_pressed() || i.pointer.any_down());
        let pressed_before = std::mem::replace(&mut self.panes.pressed_last, pressed);
        if !lost || self.modal.is_some() || self.tree.sel.is_some() || self.search.sel.is_some() || self.search.focus {
            return;
        }
        if pressed || pressed_before || !ctx.input(|i| i.focused) || eframe::egui::Popup::is_any_open(ctx) {
            return;
        }
        match self.current_tab() {
            // the mail view has its own panes keyboard, and a table grid keeps the keyboard
            // while a cell is edited or a block of cells is selected
            Some(t) if matches!(t.view, View::Mail) || t.editor.grid_has_keyboard() => return,
            _ => {}
        }
        self.focus_pane_keyboard(self.panes.focused);
    }

    /// The panes as they are saved (NAV-026).
    pub(crate) fn saved_panes(&self) -> Vec<SavedPane> {
        (0..self.panes.len())
            .map(|i| {
                let bar = self.pane_bar(i);
                let mut tabs = vec![];
                let mut active = 0;
                for (k, t) in bar.tabs.iter().enumerate() {
                    let saved = match &t.view {
                        View::Social(Some(f)) if !f.feed.is_empty() => SavedTab { feed: Some(f.feed_only()), pinned: t.pinned, ..Default::default() },
                        View::Mail => SavedTab { view: Some(SavedTab::MAIL.into()), mail: self.mail.saved_place(), pinned: t.pinned, ..Default::default() },
                        View::Calendar => {
                            let c = &self.cal.main;
                            SavedTab { view: Some(SavedTab::CALENDAR.into()), calendar: Some(SavedCalendar { mode: c.mode.name().into(), day: c.anchor, ..Default::default() }), pinned: t.pinned, ..Default::default() }
                        }
                        View::Tasks => {
                            let f = &self.tasks;
                            let saved = SavedTasks { query: f.query.clone(), show_done: f.show_done, only_due: f.only_due, ..Default::default() };
                            SavedTab { view: Some(SavedTab::TASKS.into()), tasks: Some(saved).filter(|s| *s != SavedTasks::default()), pinned: t.pinned, ..Default::default() }
                        }
                        View::Graph => {
                            let g = &self.graphs.global;
                            let saved = SavedGraph { hide_missing: !g.show_missing, hide_orphans: !g.show_orphans, ..Default::default() };
                            SavedTab { view: Some(SavedTab::GRAPH.into()), graph: Some(saved).filter(|s| *s != SavedGraph::default()), pinned: t.pinned, ..Default::default() }
                        }
                        View::Boards => SavedTab { view: Some(SavedTab::BOARDS.into()), board: self.boards.current.as_deref().map(rel_to_string), pinned: t.pinned, ..Default::default() },
                        View::SettingsFile => SavedTab { view: Some(SavedTab::SETTINGS_FILE.into()), pinned: t.pinned, ..Default::default() },
                        v => match v.path() {
                            Some(p) if matches!(v, View::Note(_)) => {
                                let place = &t.place;
                                SavedTab { file: Some(rel_to_string(p)), pinned: t.pinned, cursor: place.cursor.map(|(a, b)| [a, b]), scroll: place.scroll.map(|y| y.max(0.0) as u32), reading: t.reading, ..Default::default() }
                            }
                            Some(p) => SavedTab { file: Some(rel_to_string(p)), pinned: t.pinned, ..Default::default() },
                            None => continue,
                        },
                    };
                    if k == bar.active {
                        active = tabs.len();
                    }
                    tabs.push(saved);
                }
                SavedPane { tabs, active, weight: self.panes.weights[i], ..Default::default() }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: &[f32], b: &[f32]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01)
    }

    // spec: NAV-034
    #[test]
    fn widths_follow_the_weights_and_keep_the_minimum() {
        assert!(near(&pane_widths(&[1.0, 1.0], 1000.0, 240.0), &[500.0, 500.0]));
        assert!(near(&pane_widths(&[3.0, 1.0], 1000.0, 240.0), &[750.0, 250.0]));
        // 10 : 1 would leave the small pane at 91: it gets 240, the big one gives it up
        let w = pane_widths(&[10.0, 1.0], 1000.0, 240.0);
        assert!(near(&w, &[760.0, 240.0]), "{w:?}");
        // three panes, one big: the small ones are held at the minimum
        let w = pane_widths(&[20.0, 1.0, 1.0], 1000.0, 240.0);
        assert!((w.iter().sum::<f32>() - 1000.0).abs() < 0.1 && w[1] >= 239.9 && w[2] >= 239.9, "{w:?}");
        // no room for the minimum: equal
        assert!(near(&pane_widths(&[5.0, 1.0, 1.0], 600.0, 240.0), &[200.0, 200.0, 200.0]));
        assert!(near(&pane_widths(&[0.0, 0.0], 400.0, 100.0), &[200.0, 200.0]));
        assert!(pane_widths(&[], 400.0, 100.0).is_empty());
    }

    // spec: NAV-034
    #[test]
    fn dragging_a_border_trades_width_between_the_neighbours_only() {
        let w = drag_weights(&[1.0, 1.0, 1.0], 1, 100.0, 1200.0, 240.0);
        let widths = pane_widths(&w, 1200.0, 240.0);
        assert!(near(&widths, &[500.0, 300.0, 400.0]), "{widths:?}");
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 0.001);
        // never below the minimum, on either side
        let widths = pane_widths(&drag_weights(&[1.0, 1.0], 1, 5000.0, 1000.0, 240.0), 1000.0, 240.0);
        assert!(near(&widths, &[760.0, 240.0]), "{widths:?}");
        let widths = pane_widths(&drag_weights(&[1.0, 1.0], 1, -5000.0, 1000.0, 240.0), 1000.0, 240.0);
        assert!(near(&widths, &[240.0, 760.0]), "{widths:?}");
        // there is no border in front of the first pane
        assert_eq!(drag_weights(&[1.0, 1.0], 0, 50.0, 1000.0, 240.0), vec![1.0, 1.0]);
        // a pair without room for two minimums does not move
        assert_eq!(drag_weights(&[1.0, 1.0], 1, 50.0, 400.0, 240.0), vec![1.0, 1.0]);
    }
}
