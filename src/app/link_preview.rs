//! The link preview in the window (EDIT-033…036): opening it for the link under vim's cursor,
//! its keys, closing it when the editor moves on, drawing it, and the one place that carries out
//! what its keys ask for ([`ShaleApp::link_preview_outcome`]).

use super::{handle_editor_action, handle_reader_action, ShaleApp, View};
use crate::link_preview::{clamp_size, content_for, size_of, target_at, Grip, Open, Outcome, Target, GRAB};
use crate::{markdown, theme};
use eframe::egui::{self, Align2, Pos2, Rect, Ui, Vec2};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::Duration;

fn hash_text(text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// The frame's line: thick and in the accent colour while the preview has the keyboard.
fn stroke_of(focused: bool) -> egui::Stroke {
    egui::Stroke::new(if focused { 2.0 } else { 1.0 }, if focused { theme::palette().accent } else { theme::palette().muted })
}

impl ShaleApp {
    /// The open note of the active tab when it is in the editor: its path, the tab and the text.
    fn editing_note(&self) -> Option<(PathBuf, u64, &str)> {
        let tab = self.tabbar.tabs.get(self.tabbar.active)?;
        match &tab.view {
            View::Note(rel) if !tab.reading => Some((rel.clone(), tab.id, self.notes.get(rel)?.text.as_str())),
            _ => None,
        }
    }

    /// The keys of the open preview, before the editor sees them (EDIT-035).
    pub(super) fn link_preview_keys(&mut self, ui: &Ui) {
        let Some(open) = self.link_preview.open.as_mut() else { return };
        let outcome = ui.input_mut(|i| open.keys(i));
        self.link_preview_outcome(outcome);
    }

    /// The keys of the preview lead to one of three outcomes; this is the one place that does what they
    /// ask (Ctrl+Enter's split pane too, EDIT-036).
    pub(super) fn link_preview_outcome(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Stay => {}
            Outcome::Close => self.link_preview.open = None,
            Outcome::Open => {
                let Some(open) = self.link_preview.open.take() else { return };
                // the action of `gt` and of a click (EDIT-023)
                let from = match &open.target {
                    Target::Link { from, .. } => from.clone(),
                    Target::Web { .. } => PathBuf::new(),
                };
                handle_editor_action(&mut self.actions, open.target.follow(false), &from);
            }
            Outcome::OpenInSplit => {
                let Some(open) = self.link_preview.open.take() else { return };
                match open.target {
                    // the note opens (or is created) as Enter does, in a new pane on the right
                    Target::Link { target, heading, from } => self.in_split_right(|app| app.open_link(&target, heading, Some(from), true)),
                    // a web page has no pane: it opens in the browser, as with Enter
                    web @ Target::Web { .. } => handle_editor_action(&mut self.actions, web.follow(false), &PathBuf::new()),
                }
            }
        }
    }

    /// The action `normal.link_preview` (EDIT-033): open the preview of the link under the cursor, or
    /// when it is open already, give it the keyboard.
    pub(super) fn link_preview_key(&mut self, ctx: &egui::Context) {
        if let Some(open) = self.link_preview.open.as_mut() {
            open.focused = true;
            return;
        }
        let Some(vault) = &self.vault else { return };
        let Some((rel, tab_id, text)) = self.editing_note() else { return };
        let tab = &self.tabbar.tabs[self.tabbar.active];
        let cursor = tab.editor.vim.cursor;
        let Some(target) = target_at(text, markdown::char_to_byte(text, cursor), &rel) else {
            let tab = &mut self.tabbar.tabs[self.tabbar.active];
            tab.editor.vim.message = Some("No link under the cursor".into());
            return;
        };
        let content = content_for(ctx, &target, vault, &|r| self.notes.get(r).map(|d| d.text.clone()).or_else(|| vault.notes.get(r).map(|n| n.text.clone())), self.cfg.numbered_headings);
        let anchor = tab.editor.vim_cursor_rect.unwrap_or_else(|| Rect::from_min_size(ctx.content_rect().center(), Vec2::ZERO));
        let state = (tab_id, cursor, hash_text(text));
        if let crate::link_preview::Content::Web { url } = &content {
            let c = ctx.clone();
            self.link_preview.load(url, move || c.request_repaint());
        }
        let size = clamp_size(size_of(&self.cfg), ctx.content_rect().size());
        self.link_preview.open = Some(Open::new(target, content, state, anchor, size));
    }

    /// Does the preview still belong to what the editor shows: the same tab, text and cursor, normal mode, the keyboard?
    fn link_preview_current(&self, ctx: &egui::Context, open: &Open) -> bool {
        let Some((_, tab_id, text)) = self.editing_note() else { return false };
        let ed = &self.tabbar.tabs[self.tabbar.active].editor;
        tab_id == open.tab && ed.vim.cursor == open.cursor && ed.vim.mode == crate::vim::Mode::Normal && hash_text(text) == open.text_hash && self.normal_keys_active(ctx)
    }

    /// After the layout: take in the web page that arrived, close the preview when it does not fit the
    /// editor any more or a click went outside, and draw it.
    pub(super) fn link_preview_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if self.link_preview.poll() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        let Some(open) = self.link_preview.open.as_ref() else { return };
        let press = ctx.input(|i| if i.pointer.any_pressed() { i.pointer.interact_pos() } else { None });
        // (the handles of the edges reach a little out of the frame)
        let inside = press.zip(open.rect).is_some_and(|(p, r)| r.expand(GRAB).contains(p));
        if inside {
            // a click in the preview: it has the keyboard, and the editor keeps its focus (the press took it)
            let id = self.tabbar.tabs[self.tabbar.active].editor.id;
            ctx.memory_mut(|m| m.request_focus(id));
        } else if press.is_some() || !self.link_preview_current(&ctx, open) {
            self.link_preview.open = None;
            return;
        }
        let Some(vault) = self.vault.as_ref() else { return };
        let fonts = self.fonts.editor();
        let (base, dark) = (fonts.rendered.size, self.cfg.dark_canvas);
        let Some(open) = self.link_preview.open.as_mut() else { return };
        open.content.refresh(&ctx);
        open.focused |= inside;
        let screen = ctx.content_rect();
        open.size = clamp_size(open.size, screen.size());
        open.pad = egui::Frame::popup(ui.style()).stroke(stroke_of(open.focused)).total_margin().sum();
        let (size, pad, anchor, id) = (open.size, open.pad, open.anchor, open.id);
        let x = (anchor.min.x + open.shift_x).min(screen.right() - size.x - 8.0).max(screen.left() + 8.0);
        // below the cursor row, above it when the room below is too small and the room above is not
        let below = anchor.max.y + 4.0 + size.y <= screen.bottom() - 8.0 || anchor.min.y - 4.0 - size.y < screen.top();
        let (pos, pivot) = if below { (Pos2::new(x, anchor.max.y + 4.0), Align2::LEFT_TOP) } else { (Pos2::new(x, anchor.min.y - 4.0), Align2::LEFT_BOTTOM) };
        let stroke = stroke_of(open.focused);
        let (scenes, media) = (&mut self.scenes, &self.media);
        let lp = &mut self.link_preview;
        let mut clicked = None;
        // the handles (EDIT-037): which one is dragged, by how much, and whether the drag ended
        let mut dragged: Option<(Grip, Vec2, bool, Rect)> = None;
        let mut ended = false;
        let area = egui::Area::new(id).order(egui::Order::Foreground).fixed_pos(pos).pivot(pivot).constrain(true).show(&ctx, |ui| {
            let frame = egui::Frame::popup(ui.style()).stroke(stroke).show(ui, |ui| {
                ui.set_width(size.x - pad.x);
                clicked = crate::fonts::with_area(ui, &fonts.rendered, |ui| lp.show(ui, vault, &mut crate::reader::Caches { scenes, media }, base, dark));
            });
            for grip in Grip::ALL {
                let resp = ui.interact(grip.rect(frame.response.rect), id.with(("grip", grip.x, grip.y)), egui::Sense::drag()).on_hover_cursor(grip.cursor());
                if resp.drag_started() || resp.dragged() {
                    ui.ctx().set_cursor_icon(grip.cursor());
                    dragged = Some((grip, resp.drag_delta(), resp.drag_started(), frame.response.rect));
                }
                ended |= resp.drag_stopped();
            }
        });
        if let Some(open) = lp.open.as_mut() {
            open.rect = Some(area.response.rect);
            if let Some((grip, delta, started, rect)) = dragged {
                if started && grip.y != 0 {
                    // the frame is as big as the content; the drag starts from what is seen
                    open.size.y = clamp_size(rect.size(), screen.size()).y;
                }
                open.resizing = true;
                open.resize(grip, delta, screen.size());
                ctx.request_repaint();
            }
            if ended {
                open.resizing = false;
                let size = open.size;
                // the size is a setting that every preview uses; it is kept when the drag ends
                self.cfg.link_preview_width = size.x.round() as u32;
                self.cfg.link_preview_height = size.y.round() as u32;
                self.save_config();
            }
        }
        if let Some((a, from)) = clicked {
            let toggles = matches!(a, crate::reader::ReaderAction::ToggleTask(_));
            handle_reader_action(&mut self.actions, a, &from);
            if !toggles {
                self.link_preview_outcome(Outcome::Close);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Action, Options};
    use super::*;
    use crate::link_preview::Content;

    // spec: EDIT-036
    #[test]
    fn ctrl_enter_on_a_web_link_opens_the_browser_and_no_pane() {
        let mut app = ShaleApp::build(Options { screenshot: None, vault: None, view: None, keys: None }, false);
        let url = "https://example.org/".to_string();
        let target = Target::Web { url: url.clone() };
        app.link_preview.open = Some(Open::new(target, Content::Web { url: url.clone() }, (1, 0, 0), Rect::ZERO, Vec2::new(640.0, 480.0)));
        app.link_preview_outcome(Outcome::OpenInSplit);
        assert!(app.link_preview.open.is_none());
        assert_eq!(app.panes.len(), 1);
        assert!(matches!(app.actions.as_slice(), [Action::OpenUrl(u)] if *u == url));
    }
}
