//! Sidebars (files, search, tags, linked notes, outline, local graph), the vault-wide
//! to-do list and the dialogs.

use crate::app::{graph_click, muted, Action, LeftPanel, Modal, RightPanel, ShaleApp, View};
use crate::keys::{Action as KeyAction, Scope};
use crate::layout::{icon_button, paint_icon, Icon};
use crate::markdown;
use crate::theme;
use crate::tree_rows::{self, Row, TreeRows};
use crate::vault::{rel_to_string, FileKind};
use eframe::egui::{self, CornerRadius, FontId, Key, Modifiers, Pos2, Rect, Sense, Ui, Vec2};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

impl ShaleApp {
    pub fn left_sidebar(&mut self, ui: &mut Ui) {
        match self.left_view() {
            Some(LeftPanel::Files) => self.file_explorer(ui),
            Some(LeftPanel::Search) => self.search_panel(ui),
            Some(LeftPanel::Tags) => self.tags_panel(ui),
            None => {
                ui.label(muted("No view is here. Drag Files, Search or Tags from the right sidebar to the ribbon, or right-click its icon and choose Move or Copy."));
            }
        }
    }

    /// The view the left sidebar shows: the chosen one, or the first of Files, Search and Tags
    /// that is in the ribbon when the chosen one has moved to the right (LAY-025). None: none is.
    pub(crate) fn left_view(&self) -> Option<LeftPanel> {
        let in_ribbon = |p: LeftPanel| self.side.layout.on_left(p.item());
        [self.side.left, LeftPanel::Files, LeftPanel::Search, LeftPanel::Tags].into_iter().find(|p| in_ribbon(*p))
    }

    /// The body of a view of the right sidebar that is a view of the left one (LAY-020) or the
    /// companion of a main view (LAY-028). A copy of Files, Search or Tags draws the same state
    /// as the one on the other side (LAY-024); Mail has a state of its own (LAY-031).
    pub(crate) fn movable_view_body(&mut self, ui: &mut Ui, item: crate::sidebar::Item) {
        match item {
            crate::sidebar::Item::Files => self.file_explorer(ui),
            crate::sidebar::Item::Search => self.search_panel(ui),
            crate::sidebar::Item::Tags => self.tags_panel(ui),
            // a click on a message opens it in the Mail tab (LAY-030)
            crate::sidebar::Item::Mail => {
                if let Some(m) = self.side.panes.mail.show(ui, &self.mail, &self.cfg) {
                    self.actions.push(Action::OpenMail(Box::new(m)));
                }
            }
            // a click on an event shows it in the Calendar tab (LAY-033)
            crate::sidebar::Item::Calendar => {
                let monday = self.cfg.week_starts_monday;
                let font = self.fonts.area(crate::fonts::Area::Calendar);
                let clicked = crate::fonts::with_area(ui, &font, |ui| self.side.panes.calendar.show(ui, &self.calendar, monday));
                if let Some(o) = clicked {
                    self.actions.push(Action::OpenCalendarEvent(Box::new(o)));
                }
            }
            // a click on a post opens its conversation in the Feeds tab (LAY-034)
            crate::sidebar::Item::Social => {
                let font = self.fonts.area(crate::fonts::Area::Social);
                let clicked = crate::fonts::with_area(ui, &font, |ui| self.side.panes.social.show(ui, &mut self.social, &self.cfg.social_accounts));
                if let Some((feed, post)) = clicked {
                    self.actions.push(Action::OpenFeedPost { feed, post: Box::new(post) });
                }
            }
            _ => {}
        }
    }

    fn file_explorer(&mut self, ui: &mut Ui) {
        self.tree_keys(ui);
        // dropping anywhere on the file list (outside a folder) moves to the top of the vault
        let panel = ui.available_rect_before_wrap();
        self.drag.tree_targets.push((panel, PathBuf::new(), false));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let folder = self.current_folder();
            if icon_button(ui, Icon::NewNote, 26.0, false, "New note").clicked() {
                self.actions.push(Action::NewNote { folder: folder.clone() });
            }
            if icon_button(ui, Icon::NewFolder, 26.0, false, "New folder").clicked() {
                self.actions.push(Action::NewFolder(folder.clone()));
            }
            if icon_button(ui, Icon::Drawing, 26.0, false, "New drawing").clicked() {
                self.actions.push(Action::NewDrawing { folder: folder.clone(), embed_into: None });
            }
            if icon_button(ui, Icon::Collapse, 26.0, false, "Collapse all folders").clicked() {
                self.tree.expanded.clear();
            }
            let sort_resp = icon_button(ui, Icon::Sort, 26.0, false, &format!("Sort: {}", self.cfg.file_sort.label()));
            egui::Popup::menu(&sort_resp).show(|ui| {
                ui.set_min_width(200.0);
                for m in crate::config::FileSort::ALL {
                    if ui.radio(self.cfg.file_sort == m, m.label()).clicked() {
                        self.cfg.file_sort = m;
                        self.save_config();
                        ui.close();
                    }
                }
            });
        });
        ui.add_space(4.0);
        let vault_name = self.vault.as_ref().map(|v| self.cfg.vaults.label_of(&v.root)).unwrap_or_default();
        let bottom_h = 30.0;
        let avail = ui.available_height() - bottom_h;
        egui::ScrollArea::vertical().max_height(avail).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            // dragging near the top or bottom edge scrolls the list
            if egui::DragAndDrop::has_payload_of_type::<TreeDrag>(ui.ctx()) {
                if let Some(p) = ui.ctx().input(|i| i.pointer.latest_pos()) {
                    let clip = ui.clip_rect();
                    if clip.x_range().contains(p.x) {
                        let dy = if p.y < clip.top() + 28.0 {
                            8.0
                        } else if p.y > clip.bottom() - 28.0 {
                            -8.0
                        } else {
                            0.0
                        };
                        if dy != 0.0 {
                            ui.scroll_with_delta(Vec2::new(0.0, dy));
                            ui.ctx().request_repaint();
                        }
                    }
                }
            }
            self.tree(ui);
            // right-click on empty space
            let rest = ui.allocate_response(Vec2::new(ui.available_width(), (ui.available_height()).max(40.0)), Sense::click());
            rest.context_menu(|ui| {
                if ui.button("New note").clicked() {
                    self.actions.push(Action::NewNote { folder: PathBuf::new() });
                    ui.close();
                }
                if ui.button("New folder").clicked() {
                    self.actions.push(Action::NewFolder(PathBuf::new()));
                    ui.close();
                }
                if ui.button("New drawing").clicked() {
                    self.actions.push(Action::NewDrawing { folder: PathBuf::new(), embed_into: None });
                    ui.close();
                }
                ui.separator();
                if ui.button("Export to PDF…").clicked() {
                    self.actions.push(Action::ExportFolder(PathBuf::new()));
                    ui.close();
                }
                if ui.add_enabled(self.can_restore_trashed(), egui::Button::new("Restore last deleted")).clicked() {
                    self.actions.push(Action::RestoreTrashed);
                    ui.close();
                }
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            let path = self.vault.as_ref().map(|v| v.root.to_string_lossy().into_owned()).unwrap_or_default();
            if crate::vault_menu::vault_label(ui, &vault_name, &path) {
                self.open_vault_menu();
            }
        });
    }

    /// The rows the file list shows right now, top to bottom (PERF-011): made again only when the
    /// vault, the sort setting or the open folders changed.
    pub(crate) fn tree_rows(&mut self) -> Arc<Vec<Row>> {
        match &self.vault {
            Some(v) => self.tree.rows.get(v, self.cfg.file_sort, &self.tree.expanded),
            None => Arc::default(),
        }
    }

    /// The rows the file list shows right now, top to bottom: (path, is a folder).
    pub(crate) fn visible_tree_rows(&self) -> Vec<(PathBuf, bool)> {
        let Some(v) = &self.vault else { return vec![] };
        tree_rows::build(v, self.cfg.file_sort, &self.tree.expanded).into_iter().map(|r| (r.rel.clone(), r.is_folder())).collect()
    }

    /// Keys while the file list has the keyboard: j / k files, Ctrl+J / Ctrl+K folders, Space
    /// opens or closes a folder, Enter opens, Esc back to the editor.
    fn tree_keys(&mut self, ui: &Ui) {
        let Some(sel) = self.tree.sel.clone() else { return };
        if ui.ctx().memory(|m| m.focused().is_some()) {
            // something else took the keyboard (a click into the editor, a dialog)
            self.tree.sel = None;
            return;
        }
        let rows = self.tree_rows();
        let Some(cur) = rows.iter().position(|r| r.rel == sel).or(if rows.is_empty() { None } else { Some(0) }) else {
            self.tree.sel = None;
            return;
        };
        let (action, enter, esc) = ui.input_mut(|i| (self.keymap.poll(i, Scope::FileList), i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape)));
        let is = |k: KeyAction| action == Some(k);
        let (next, prev, next_dir, prev_dir, space) = (is(KeyAction::TreeNextFile), is(KeyAction::TreePrevFile), is(KeyAction::TreeNextFolder), is(KeyAction::TreePrevFolder), is(KeyAction::TreeToggleFolder));
        // the next row of the wanted kind in that direction (stays put at the ends)
        let step = |down: bool, dirs: bool| -> usize {
            let pick = |i: &usize| rows[*i].is_folder() == dirs;
            let found = if down { (cur + 1..rows.len()).find(pick) } else { (0..cur).rev().find(pick) };
            found.unwrap_or(cur)
        };
        let new = if next_dir {
            step(true, true)
        } else if prev_dir {
            step(false, true)
        } else if next {
            step(true, false)
        } else if prev {
            step(false, false)
        } else {
            cur
        };
        let (path, is_dir) = (rows[new].rel.clone(), rows[new].is_folder());
        self.tree.scroll |= new != cur;
        self.tree.sel = Some(path.clone());
        if (space || enter) && is_dir {
            if !self.tree.expanded.remove(&path) {
                self.tree.expanded.insert(path);
            }
        } else if enter {
            self.actions.push(Action::Open { rel: path, new_tab: false });
            self.focus_editor();
        } else if esc {
            self.focus_editor();
        }
    }

    /// Outline the row selected by the keyboard (and scroll to it once after it changed).
    fn mark_tree_sel(&mut self, ui: &Ui, rel: &Path, resp: &egui::Response) {
        if self.tree.sel.as_deref() == Some(rel) {
            ui.painter().rect_stroke(resp.rect, CornerRadius::same(4), egui::Stroke::new(1.5, theme::palette().accent), egui::StrokeKind::Inside);
            if std::mem::take(&mut self.tree.scroll) {
                resp.scroll_to_me(Some(egui::Align::Center));
            }
        }
    }

    /// The file list: only the rows in view (and a margin) are drawn, the others are one empty
    /// space each side, as all rows are as high as one another (PERF-011).
    fn tree(&mut self, ui: &mut Ui) {
        let rows = self.tree_rows();
        let current = self.current_path();
        let pitch = TREE_ROW_H + ui.spacing().item_spacing.y;
        let top = ui.cursor().min.y;
        let clip = ui.clip_rect();
        let rows_at = |y: f32| ((y - top) / pitch).max(0.0);
        let first = (rows_at(clip.min.y - TREE_MARGIN).floor() as usize).min(rows.len());
        let end = (rows_at(clip.max.y + TREE_MARGIN).ceil() as usize).min(rows.len()).max(first);
        if self.tree.scroll {
            // the selected row is out of the drawn rows: scroll to where it would be; it is
            // drawn (and outlined) in the frame after
            let at = self.tree.sel.as_ref().and_then(|s| rows.iter().position(|r| r.rel == *s));
            if let Some(i) = at.filter(|i| !(first..end).contains(i)) {
                let y = top + i as f32 * pitch;
                ui.scroll_to_rect(Rect::from_min_size(Pos2::new(clip.min.x, y), Vec2::new(clip.width(), TREE_ROW_H)), Some(egui::Align::Center));
                ui.ctx().request_repaint();
            }
        }
        if first > 0 {
            ui.add_space(first as f32 * pitch);
        }
        for row in &rows[first..end] {
            if row.is_folder() {
                self.tree_folder(ui, row);
            } else {
                self.tree_file(ui, row, current.as_deref() == Some(row.rel.as_path()));
            }
        }
        if end < rows.len() {
            ui.add_space((rows.len() - end) as f32 * pitch);
        }
    }

    fn tree_folder(&mut self, ui: &mut Ui, row: &Row) {
        {
            let f = row.rel.clone();
            let open = self.tree.expanded.contains(&f);
            let resp = tree_row(ui, row.depth, &row.name, Some(open), false, theme::palette().text, "");
            self.mark_tree_sel(ui, &f, &resp);
            self.drag.add_drop_zone(resp.rect.intersect(ui.clip_rect()), f.clone(), None);
            self.drag.tree_targets.push((resp.rect.intersect(ui.clip_rect()), f.clone(), !open));
            if resp.drag_started() {
                egui::DragAndDrop::set_payload(ui.ctx(), TreeDrag(f.clone()));
            }
            if resp.clicked() {
                if open {
                    self.tree.expanded.remove(&f);
                } else {
                    self.tree.expanded.insert(f.clone());
                }
            }
            resp.context_menu(|ui| {
                if ui.button("New note here").clicked() {
                    self.actions.push(Action::NewNote { folder: f.clone() });
                    ui.close();
                }
                if ui.button("New drawing here").clicked() {
                    self.actions.push(Action::NewDrawing { folder: f.clone(), embed_into: None });
                    ui.close();
                }
                if ui.button("New folder here").clicked() {
                    self.actions.push(Action::NewFolder(f.clone()));
                    ui.close();
                }
                ui.separator();
                if ui.button("Export to PDF…").clicked() {
                    self.actions.push(Action::ExportFolder(f.clone()));
                    ui.close();
                }
                if ui.button("Rename…").clicked() {
                    self.actions.push(Action::Rename(f.clone()));
                    ui.close();
                }
                if ui.button("Move to trash").clicked() {
                    self.actions.push(Action::Trash(f.clone()));
                    ui.close();
                }
            });
        }
    }

    fn tree_file(&mut self, ui: &mut Ui, row: &Row, active: bool) {
        {
            let (rel, kind) = (row.rel.clone(), row.kind.unwrap_or(FileKind::Other));
            let color = if kind == FileKind::Other { theme::palette().muted } else { theme::palette().text };
            let resp = tree_row(ui, row.depth, &row.name, None, active, color, &row.badge);
            self.mark_tree_sel(ui, &rel, &resp);
            // dropping on a file puts the dragged thing next to it
            self.drag.tree_targets.push((resp.rect.intersect(ui.clip_rect()), rel.parent().map(|p| p.to_path_buf()).unwrap_or_default(), false));
            if resp.drag_started() {
                egui::DragAndDrop::set_payload(ui.ctx(), TreeDrag(rel.clone()));
            }
            self.drag.add_drop_zone(resp.rect.intersect(ui.clip_rect()), rel.parent().map(|p| p.to_path_buf()).unwrap_or_default(), (kind == FileKind::Note).then(|| rel.clone()));
            if resp.clicked() {
                let new_tab = ui.input(|i| i.modifiers.command);
                self.actions.push(Action::Open { rel: rel.clone(), new_tab });
            }
            if resp.middle_clicked() {
                self.actions.push(Action::Open { rel: rel.clone(), new_tab: true });
            }
            resp.context_menu(|ui| {
                if ui.button("Open in new tab").clicked() {
                    self.actions.push(Action::Open { rel: rel.clone(), new_tab: true });
                    ui.close();
                }
                if kind != FileKind::Other && ui.button("Export to PDF…").clicked() {
                    self.actions.push(Action::Export(rel.clone()));
                    ui.close();
                }
                if kind == FileKind::Note && ui.button("Export to Word…").clicked() {
                    self.actions.push(Action::ExportDocx(rel.clone()));
                    ui.close();
                }
                if ui.button("Rename…").clicked() {
                    self.actions.push(Action::Rename(rel.clone()));
                    ui.close();
                }
                if ui.button("Copy link").clicked() {
                    let link = self.vault.as_ref().and_then(|v| v.file(&rel).map(|f| format!("[[{}]]", f.link_name))).unwrap_or_default();
                    ui.ctx().copy_text(link);
                    ui.close();
                }
                ui.separator();
                if ui.button("Move to trash").clicked() {
                    self.actions.push(Action::Trash(rel.clone()));
                    ui.close();
                }
            });
        }
    }

    fn tags_panel(&mut self, ui: &mut Ui) {
        ui.label(egui::RichText::new("Tags").size(13.0).color(theme::palette().muted));
        ui.add_space(4.0);
        let Some(v) = &self.vault else { return };
        let tags = v.all_tags();
        if tags.is_empty() {
            ui.label(muted("No #tags in this vault yet."));
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (tag, n) in tags {
                ui.horizontal(|ui| {
                    if ui.add(egui::Label::new(egui::RichText::new(format!("#{tag}")).color(theme::palette().tag)).sense(Sense::click())).on_hover_text("Search for this tag").clicked() {
                        self.search.query = format!("#{tag}");
                        self.side.left = LeftPanel::Search;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(muted(n.to_string()).size(12.0));
                    });
                });
            }
        });
    }

    /// The panel of one view of the right sidebar (LAY-003).
    pub fn right_panel_body(&mut self, ui: &mut Ui, panel: RightPanel) {
        if panel == RightPanel::Calendar {
            let monday = self.cfg.week_starts_monday;
            let mut act = None;
            let area = ui.available_rect_before_wrap();
            self.drag.add_calendar_area(area);
            egui::ScrollArea::vertical().id_salt("side-calendar").auto_shrink([false, false]).show(ui, |ui| {
                let font = self.fonts.area(crate::fonts::Area::Calendar);
                act = crate::fonts::with_area(ui, &font, |ui| self.cal.side.show(ui, &self.calendar, monday, false));
            });
            if let Some(a) = act {
                self.calendar_action(a);
            }
            return;
        }
        if panel == RightPanel::History {
            self.history_panel_body(ui);
            return;
        }
        if panel == RightPanel::Thread {
            self.thread_panel_body(ui);
            return;
        }
        let Some(rel) = self.current_path() else {
            ui.label(muted("Open a note to see its links."));
            return;
        };
        let Some(v) = &self.vault else { return };
        let name = v.file(&rel).map(|f| f.display_name()).unwrap_or_default();
        match panel {
            RightPanel::Backlinks => {
                let bl = self.vault_cache.backlinks(v, &rel);
                let um = self.vault_cache.unlinked(v, &rel);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    section_header(ui, &format!("Linked mentions  {}", bl.len()));
                    if bl.is_empty() {
                        ui.label(muted(format!("No notes link to “{name}” yet.")).size(12.5));
                    }
                    for (src, lines) in bl.iter() {
                        self.mention_block(ui, src, lines);
                    }
                    ui.add_space(10.0);
                    section_header(ui, &format!("Unlinked mentions  {}", um.len()));
                    for (src, lines) in um.iter() {
                        self.mention_block(ui, src, lines);
                    }
                });
            }
            RightPanel::Outgoing => {
                let out = v.outgoing(&rel);
                let outside = v.outgoing_outside(&rel);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let (ok, missing): (Vec<_>, Vec<_>) = out.into_iter().partition(|o| o.2.is_some());
                    section_header(ui, &format!("Links  {}", ok.len()));
                    for (display, _target, r) in ok {
                        if let Some(r) = r {
                            let resp = ui.add(egui::Label::new(egui::RichText::new(display).color(theme::palette().link)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
                            if resp.clicked() {
                                self.actions.push(Action::Open { rel: r, new_tab: false });
                            }
                        }
                    }
                    if !outside.is_empty() {
                        // media outside the vault: no notes, nothing to create (MEDIA-016)
                        ui.add_space(10.0);
                        section_header(ui, &format!("Outside the vault  {}", outside.len()));
                        for (display, target) in outside {
                            let resp = ui
                                .add(egui::Label::new(egui::RichText::new(display).color(theme::palette().link)).sense(Sense::click()))
                                .on_hover_text("Click to open it with your system's program")
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            if resp.clicked() {
                                self.actions.push(Action::OpenLink { target, heading: None, from: Some(rel.clone()), new_tab: false });
                            }
                        }
                    }
                    ui.add_space(10.0);
                    section_header(ui, &format!("Not created yet  {}", missing.len()));
                    for (display, target, _) in missing {
                        // a path link whose name exists in other places: offer them (LINK-012)
                        let candidates = v.fix_candidates(&target, Some(&rel));
                        ui.horizontal_wrapped(|ui| {
                            let resp =
                                ui.add(egui::Label::new(egui::RichText::new(display).color(theme::palette().link_missing)).sense(Sense::click())).on_hover_text("Click to create this note").on_hover_cursor(egui::CursorIcon::PointingHand);
                            if resp.clicked() {
                                self.actions.push(Action::OpenLink { target: target.clone(), heading: None, from: Some(rel.clone()), new_tab: false });
                            }
                            if !candidates.is_empty() {
                                ui.menu_button(egui::RichText::new("Fix link").small(), |ui| {
                                    ui.label(muted("Point the link at:").size(12.0));
                                    for c in &candidates {
                                        if ui.button(markdown::strip_md_ext(&rel_to_string(c))).clicked() {
                                            self.actions.push(Action::FixLink { note: rel.clone(), target: target.clone(), to: c.clone() });
                                            ui.close();
                                        }
                                    }
                                })
                                .response
                                .on_hover_text(format!("No file at this path; {} file(s) have this name", candidates.len()));
                            }
                        });
                    }
                });
            }
            RightPanel::Outline => {
                let text = self.notes.get(&rel).map(|d| d.text.clone()).or_else(|| v.notes.get(&rel).map(|n| n.text.clone())).unwrap_or_default();
                let numbered = markdown::headings_numbered(&text, self.cfg.numbered_headings);
                let heads = markdown::heading_numbers(&text);
                // a reference in a heading is a footnote mark when the note defines it (EDIT-050)
                let _defined = markdown::DefinedFootnotes::set(markdown::find_footnotes(&text).defined());
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if heads.is_empty() {
                        ui.label(muted("No headings in this note."));
                    }
                    let min = heads.iter().map(|h| h.1).min().unwrap_or(1);
                    for (line, level, title, number) in heads {
                        ui.horizontal(|ui| {
                            ui.add_space((level - min) as f32 * 14.0);
                            if let Some(n) = number.filter(|_| numbered) {
                                ui.label(muted(n).size(14.0));
                            }
                            let links: Vec<_> = markdown::find_links(&title)
                                .into_iter()
                                .map(|l| {
                                    let ok = v.exists(&l.target, Some(&rel));
                                    (l, ok)
                                })
                                .collect();
                            let job = crate::editor::outline_job(&title, &links, 14.0);
                            if crate::editor::outline_label(ui, job, 14.0).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                self.actions.push(Action::GotoHeading { rel: rel.clone(), line });
                            }
                        });
                    }
                });
            }
            RightPanel::Calendar | RightPanel::History | RightPanel::Thread => {}
            RightPanel::LocalGraph => {
                ui.horizontal(|ui| {
                    ui.label(muted("Depth"));
                    let mut d = self.graphs.local.depth as i32;
                    if ui.add(egui::Slider::new(&mut d, 1..=3)).changed() {
                        self.graphs.local.depth = d as usize;
                        self.graphs.local.focus = None;
                    }
                });
                if self.graphs.local.focus.as_deref() != Some(rel.as_path()) {
                    self.graphs.local.focus = Some(rel.clone());
                }
                if let Some(c) = self.graphs.local.ui(ui, v, Some(&rel), true) {
                    graph_click(&mut self.actions, c);
                }
            }
        }
    }

    fn mention_block(&mut self, ui: &mut Ui, src: &Path, lines: &[(usize, String)]) {
        let Some(v) = &self.vault else { return };
        let name = v.file(src).map(|f| f.display_name()).unwrap_or_default();
        if ui.add(egui::Label::new(egui::RichText::new(&name).color(theme::palette().text_strong)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.actions.push(Action::Open { rel: src.to_path_buf(), new_tab: false });
        }
        for (line, text) in lines.iter().take(5) {
            let shown: String = text.chars().take(160).collect();
            let resp = egui::Frame::new().fill(theme::palette().bg_primary).corner_radius(CornerRadius::same(4)).inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(egui::RichText::new(shown).size(12.5).color(theme::palette().muted)).wrap());
            });
            if resp.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Go to this line").clicked() {
                self.actions.push(Action::GotoLine { rel: src.to_path_buf(), line: *line });
            }
        }
        ui.add_space(6.0);
    }

    pub fn tasks_view(&mut self, ui: &mut Ui) {
        let Some(v) = &self.vault else { return };
        let all = self.vault_cache.tasks(v);
        let open = self.vault_cache.open_tasks(v);
        let done = all.len() - open;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let full = ui.available_width();
            let col = (full - 32.0).clamp(200.0, 860.0);
            let pad = ((full - col) / 2.0).max(16.0);
            ui.horizontal(|ui| {
                ui.add_space(pad);
                ui.vertical(|ui| {
                    ui.set_width(col - 16.0);
                    ui.add_space(14.0);
                    ui.label(egui::RichText::new("Tasks").size(28.0).color(theme::palette().text_strong));
                    ui.label(muted(format!("{open} open · {done} done · from every note in the vault")));
                    ui.add_space(10.0);
                    // progress bar
                    let frac = if all.is_empty() { 0.0 } else { done as f32 / all.len() as f32 };
                    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
                    ui.painter().rect_filled(r, CornerRadius::same(3), theme::palette().bg_hover);
                    ui.painter().rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width() * frac, r.height())), CornerRadius::same(3), theme::palette().accent);
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.tasks.query).hint_text("Filter tasks or #tags…").desired_width(260.0));
                        ui.checkbox(&mut self.tasks.show_done, "Show done");
                        ui.checkbox(&mut self.tasks.only_due, "Only with due date");
                    });
                    ui.add_space(10.0);
                    let q = self.tasks.query.to_lowercase();
                    let mut current_note: Option<&PathBuf> = None;
                    let mut list: Vec<_> = all
                        .iter()
                        .filter(|(_, t)| self.tasks.show_done || !t.done)
                        .filter(|(_, t)| !self.tasks.only_due || t.due.is_some())
                        .filter(|(p, t)| q.is_empty() || t.text.to_lowercase().contains(&q) || rel_to_string(p).to_lowercase().contains(&q))
                        .collect();
                    if self.tasks.only_due {
                        list.sort_by(|a, b| a.1.due.cmp(&b.1.due));
                    }
                    // the rows: a heading where the note changes, then its tasks
                    let mut rows: Vec<(&PathBuf, Option<&markdown::Task>)> = Vec::with_capacity(list.len() * 2);
                    for (rel, t) in list {
                        if !self.tasks.only_due && current_note != Some(rel) {
                            current_note = Some(rel);
                            rows.push((rel, None));
                        }
                        rows.push((rel, Some(t)));
                    }
                    let shown = rows.iter().filter(|r| r.1.is_some()).count();
                    let only_due = self.tasks.only_due;
                    let actions = &mut self.actions;
                    self.tasks.rows.show(ui, rows.len(), 24.0, |ui, i| {
                        let (rel, task) = rows[i];
                        let Some(t) = task else {
                            ui.add_space(8.0);
                            let name = v.file(rel).map(|f| f.display_name()).unwrap_or_default();
                            let folder = rel.parent().map(rel_to_string).filter(|s| !s.is_empty());
                            ui.horizontal(|ui| {
                                if ui.add(egui::Label::new(egui::RichText::new(&name).size(15.0).color(theme::palette().text_strong)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    actions.push(Action::Open { rel: rel.clone(), new_tab: false });
                                }
                                if let Some(f) = folder {
                                    ui.label(muted(f).size(12.0));
                                }
                            });
                            return;
                        };
                        ui.push_id((rel, t.line), |ui| {
                            let row = ui.horizontal(|ui| {
                                ui.add_space(t.indent as f32 * 8.0);
                                let mut d = t.done;
                                if ui.checkbox(&mut d, "").on_hover_text("Tick off (saved to the note)").clicked() {
                                    actions.push(Action::ToggleTask { rel: rel.clone(), offset: t.check_offset });
                                }
                                let shown = strip_due(&t.text);
                                let (link, plain) = crate::reader::inline_line(ui, &shown, v, rel, 14.0, t.done);
                                if let Some(a) = link {
                                    crate::app::handle_reader_action(actions, a, rel);
                                } else if plain {
                                    actions.push(Action::GotoLine { rel: rel.clone(), line: t.line });
                                }
                                if let Some(due) = &t.due {
                                    let overdue = today().map(|d| due.as_str() < d.as_str()).unwrap_or(false) && !t.done;
                                    ui.label(egui::RichText::new(format!("due {due}")).size(12.0).color(if overdue { theme::palette().link_missing } else { theme::palette().accent }));
                                }
                                if only_due {
                                    ui.label(muted(v.file(rel).map(|f| f.display_name()).unwrap_or_default()).size(12.0));
                                }
                            });
                            if ui.rect_contains_pointer(row.response.rect) {
                                crate::reader::note_hovered_task(ui.ctx(), Some(rel.clone()), t.check_offset);
                            }
                        });
                    });
                    if shown == 0 {
                        ui.add_space(20.0);
                        ui.label(muted(if all.is_empty() { "No tasks yet. Write “- [ ] something” in any note." } else { "Nothing matches. Everything done? 🎉" }));
                    }
                    ui.add_space(40.0);
                });
            });
        });
    }

    pub fn modals(&mut self, ui: &mut Ui) {
        let Some(mut modal) = self.modal.take() else { return };
        let mut keep = true;
        let mut open_calendars = false;
        let mut open_usage = false;
        let mut open_history = false;
        let mut open_guide = false;
        let mut open_mail = false;
        let mut open_social = false;
        let mut open_tex_setup = false;
        let ctx = ui.ctx().clone();
        match &mut modal {
            Modal::Picker(p) => {
                let rendered = self.search_rendered();
                if let Some(v) = &self.vault {
                    let data = crate::search_all::Ctx { vault: v, mail: self.mail.index(), posts: &self.social.index };
                    let out = crate::picker::show(&ctx, p, &data, &mut self.search_worker, &self.keymap, &mut self.cfg.search, rendered);
                    if out.save {
                        self.save_config();
                    }
                    keep = out.pick.is_none() && !out.close;
                    if let Some(hit) = out.pick {
                        // a note opens in this tab and the keyboard goes into it; a mail or a post has its own view
                        let note = matches!(hit.action, Action::Open { .. } | Action::GotoMatch { .. });
                        self.actions.push(hit.action);
                        if note {
                            self.actions.push(Action::FocusEditor);
                        }
                    }
                }
            }
            Modal::QuickSwitch { query, sel } => {
                let resp = crate::dialog::Dialog::new("quick-switch", "Open a note").width(520.0).show(&ctx, |ui| {
                    let r = ui.add(egui::TextEdit::singleline(query).hint_text("Find or create a note…").desired_width(f32::INFINITY).font(FontId::proportional(16.0)));
                    r.request_focus();
                    let Some(v) = &self.vault else { return };
                    let mut items: Vec<(i32, PathBuf, String)> = v.files.iter().filter_map(|f| markdown::fuzzy_score(query, &rel_to_string(&f.rel)).map(|s| (s, f.rel.clone(), f.display_name()))).collect();
                    items.sort_by(|a, b| b.0.cmp(&a.0).then(a.2.cmp(&b.2)));
                    items.truncate(12);
                    let create = !query.trim().is_empty() && !items.iter().any(|(_, _, n)| crate::vault::name_key(n) == crate::vault::name_key(query.trim()));
                    let n = items.len() + create as usize;
                    let keys = crate::picker::ListKeys::take(ui, &self.keymap);
                    keys.select(sel, n);
                    let mut pick: Option<usize> = if keys.enter && n > 0 { Some(*sel) } else { None };
                    for (i, (_, rel, name)) in items.iter().enumerate() {
                        let folder = rel.parent().map(rel_to_string).unwrap_or_default();
                        let label = if folder.is_empty() { name.clone() } else { format!("{name}    {folder}") };
                        if ui.selectable_label(i == *sel, label).clicked() {
                            pick = Some(i);
                        }
                    }
                    if create && ui.selectable_label(*sel == items.len(), egui::RichText::new(format!("＋ Create “{}”", query.trim())).color(theme::palette().link_missing)).clicked() {
                        pick = Some(items.len());
                    }
                    crate::theme::prose(ui, muted(crate::picker::ListKeys::hint(&self.keymap)).size(11.5));
                    if let Some(p) = pick {
                        if p < items.len() {
                            self.actions.push(Action::Open { rel: items[p].1.clone(), new_tab: false });
                        } else {
                            self.actions.push(Action::OpenLink { target: query.trim().to_string(), heading: None, from: None, new_tab: false });
                        }
                        keep = false;
                    }
                    if keys.esc {
                        keep = false;
                    }
                });
                if resp.should_close() {
                    keep = false;
                }
            }
            Modal::Rename { rel, name, error } => {
                let resp = crate::dialog::Dialog::new("rename", "Rename").width(420.0).show(&ctx, |ui| {
                    let r = ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                    r.request_focus();
                    if self.cfg.update_links_on_rename {
                        crate::theme::hint(ui, "Links in other notes will be updated.");
                    }
                    if let Some(e) = error {
                        crate::theme::problem(ui, e.as_str());
                    }
                    let enter = ui.input(|i| i.key_pressed(Key::Enter));
                    ui.horizontal(|ui| {
                        if ui.button("Rename").clicked() || enter {
                            let r2 = rel.clone();
                            let n2 = name.clone();
                            match self.rename(&r2, &n2) {
                                Ok(()) => keep = false,
                                Err(e) => *error = Some(e),
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
                if resp.should_close() {
                    keep = false;
                }
            }
            Modal::NewFolder { parent, name } => {
                let resp = crate::dialog::Dialog::new("new-folder", "New folder").width(380.0).show(&ctx, |ui| {
                    let r = ui.add(egui::TextEdit::singleline(name).hint_text("Folder name").desired_width(f32::INFINITY));
                    r.request_focus();
                    let enter = ui.input(|i| i.key_pressed(Key::Enter));
                    ui.horizontal(|ui| {
                        if (ui.button("Create").clicked() || enter) && !name.trim().is_empty() {
                            match self.create_folder(parent, name) {
                                Ok(rel) => {
                                    let mut p = Some(rel.as_path());
                                    while let Some(dir) = p.filter(|d| !d.as_os_str().is_empty()) {
                                        self.tree.expanded.insert(dir.to_path_buf());
                                        p = dir.parent();
                                    }
                                }
                                Err(e) => self.toasts.push((format!("Could not create folder: {e}"), std::time::Instant::now())),
                            }
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
                if resp.should_close() {
                    keep = false;
                }
            }
            Modal::ConfirmTrash { rel } => {
                let resp = crate::dialog::Dialog::new("trash", "Move to trash?").width(400.0).show(&ctx, |ui| {
                    crate::theme::paragraph(ui, format!("“{}” will be moved to the .trash folder inside your vault. You can get it back from there.", rel_to_string(rel)));
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("Move to trash").color(theme::palette().link_missing)).clicked() {
                            let r = rel.clone();
                            self.trash(&r);
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
                if resp.should_close() {
                    keep = false;
                }
            }
            Modal::LiftPaste { text } => {
                let resp = crate::dialog::Dialog::new("lift-paste", "Lift headings?").width(440.0).show(&ctx, |ui| {
                    crate::theme::paragraph(ui, "The pasted text has no # heading. Lift its headings?");
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("Lift").strong()).on_hover_text("## becomes #, ### becomes ##, and so on").clicked() {
                            let lifted = markdown::lift_headings(text).unwrap_or_else(|| text.clone());
                            self.actions.push(Action::PasteText(lifted));
                            keep = false;
                        }
                        if ui.button("Paste as it is").clicked() {
                            self.actions.push(Action::PasteText(std::mem::take(text)));
                            keep = false;
                        }
                    });
                });
                if resp.should_close() && keep {
                    self.actions.push(Action::PasteText(std::mem::take(text)));
                    keep = false;
                }
            }
            Modal::OutsideFiles { text } => {
                // the keyboard belongs to the question, not to the note behind it
                if let Some(id) = ctx.memory(|m| m.focused()) {
                    ctx.memory_mut(|m| m.surrender_focus(id));
                }
                let names: Vec<String> = crate::media::pasted_files(text).unwrap_or_default().iter().filter_map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
                let mut answer = None;
                let resp = crate::dialog::Dialog::new("outside-files", "Files from outside the vault").width(460.0).show(&ctx, |ui| {
                    crate::theme::paragraph(ui, if names.len() == 1 { format!("“{}” is outside the vault.", names[0]) } else { format!("{} files outside the vault: {}.", names.len(), names.join(", ")) });
                    crate::theme::hint(ui, "A copy goes to the media folder and is synced with the vault. A link points to the file where it is: nothing is copied, and the file is not synced.");
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("Copy into the vault").strong()).on_hover_text("C or Enter").clicked() {
                            answer = Some(Some(crate::media::intake::Choice::Copy));
                        }
                        if ui.button("Link where they are").on_hover_text("L").clicked() {
                            answer = Some(Some(crate::media::intake::Choice::Link));
                        }
                        if ui.button("Cancel").on_hover_text("Esc").clicked() {
                            answer = Some(None);
                        }
                    });
                    ui.label(egui::RichText::new("C copy · L link · Esc cancel").small().color(theme::palette().faint));
                    ui.input_mut(|i| {
                        if i.consume_key(Modifiers::NONE, Key::C) || i.consume_key(Modifiers::NONE, Key::Enter) {
                            answer = Some(Some(crate::media::intake::Choice::Copy));
                        } else if i.consume_key(Modifiers::NONE, Key::L) {
                            answer = Some(Some(crate::media::intake::Choice::Link));
                        }
                    });
                });
                if resp.should_close() && answer.is_none() {
                    answer = Some(None);
                }
                if let Some(how) = answer {
                    if let Some(how) = how {
                        self.actions.push(Action::PasteFiles(std::mem::take(text), how));
                    }
                    keep = false;
                }
            }
            Modal::Vaults(_)
            | Modal::TexSetup
            | Modal::Export(_)
            | Modal::ExportDocx(_)
            | Modal::Calendars(_)
            | Modal::Event(_)
            | Modal::Import(_)
            | Modal::ImportSummary(_)
            | Modal::MailAccounts(_)
            | Modal::Social(_)
            | Modal::DataUsage
            | Modal::History
            | Modal::Changes
            | Modal::Conflict(_)
            | Modal::SyncWizard => {}
            Modal::ConvertCanvas { rel, new_tab, always, existing } => {
                let resp = crate::dialog::Dialog::new("convert-canvas", "Convert to an editable drawing?").width(460.0).show(&ctx, |ui| {
                    let name = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    crate::theme::paragraph(ui, format!("“{name}” is an Obsidian Canvas. Shale shows canvases read-only."));
                    match existing.as_ref() {
                        Some(copy) => ui.label(format!("It was converted before: “{}”.", rel_to_string(copy))),
                        None => {
                            crate::theme::paragraph(ui, format!("Converting makes the drawing “{}” next to it (Obsidian opens it with the Excalidraw plugin). The canvas file is not changed.", crate::drawing::canvas::drawing_name_for(rel)))
                        }
                    };
                    ui.checkbox(always, "Always convert canvases when opening them").on_hover_text("Also in Settings → Drawings");
                    ui.horizontal(|ui| {
                        let convert = if existing.is_some() { "Open the drawing" } else { "Convert" };
                        if ui.button(convert).clicked() {
                            if *always {
                                self.cfg.auto_convert_canvas = true;
                                self.save_config();
                            }
                            let (r, t) = (rel.clone(), *new_tab);
                            match existing.clone() {
                                Some(copy) => self.actions.push(Action::Open { rel: copy, new_tab: t }),
                                None => self.actions.push(Action::ConvertCanvas { rel: r, new_tab: t }),
                            }
                            keep = false;
                        }
                        if existing.is_some() && ui.button("Convert again").on_hover_text("Make another drawing from the canvas as it is now").clicked() {
                            self.actions.push(Action::ConvertCanvas { rel: rel.clone(), new_tab: *new_tab });
                            keep = false;
                        }
                        let ro = ui.add_enabled(!*always, egui::Button::new("Open read-only")).on_disabled_hover_text("Untick “Always convert” to open it read-only");
                        if ro.clicked() {
                            let (r, t) = (rel.clone(), *new_tab);
                            self.open_canvas_read_only(&r, t);
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
                if resp.should_close() {
                    keep = false;
                }
            }
            Modal::Settings { vault_path, tab, keys, find } => {
                let mut asks = crate::settings::Asks::default();
                // the header (the search, CFG-072) and the page both work on the window's state
                let mut st = (&mut *self, vault_path, tab, keys, find, &mut asks, false);
                let resp = crate::dialog::Dialog::new("settings", "Settings").width(740.0).show_with_header_and_footer(
                    &ctx,
                    &mut st,
                    |ui, (app, _, tab, keys, find, asks, _)| app.settings_header(ui, tab, keys, find, asks),
                    |ui, (app, vault_path, tab, keys, _, asks, _)| app.settings_body(ui, vault_path, tab, keys, asks),
                    |ui, st| {
                        if ui.button("Done").clicked() {
                            st.6 = true;
                        }
                    },
                );
                let done = st.6;
                if done || asks.close {
                    keep = false;
                }
                // closing Settings stops what it started (JOB-010): an install, an update, an import, a check
                if done || resp.should_close() {
                    self.sync.managed.stop_jobs();
                    self.tex.stop_jobs();
                }
                open_calendars |= asks.calendars;
                open_usage |= asks.usage;
                open_history |= asks.history_deleted;
                open_guide |= asks.guide;
                open_tex_setup |= asks.tex_setup;
                open_mail |= asks.mail;
                open_social |= asks.social;
                if resp.should_close() {
                    self.keymap.recording = false;
                    keep = false;
                }
            }
        }
        if open_tex_setup {
            self.open_tex_setup(None, crate::pdf::tex::setup::Return::Settings);
            keep = false;
        }
        if open_mail {
            self.open_mail_settings();
            keep = false;
        }
        if open_social {
            self.open_social_accounts();
            keep = false;
        }
        if open_calendars {
            self.open_calendar_settings();
            keep = false;
        }
        if open_history {
            self.open_history_deleted();
            keep = false;
        }
        if open_usage {
            self.open_data_usage();
            keep = false;
        }
        if open_guide {
            self.open_user_guide();
            keep = false;
        }
        if let Modal::History = &modal {
            keep = self.history_dialog(&ctx);
        }
        if let Modal::Changes = &modal {
            keep = self.history_changes_dialog(&ctx);
        }
        if let Modal::Conflict(v) = &mut modal {
            keep &= self.conflict_dialog(&ctx, v);
        }
        if let Modal::SyncWizard = &modal {
            keep = self.sync_wizard_dialog(&ctx);
        }
        if let Modal::DataUsage = &modal {
            // the state both parts work on, and what the footer asked for
            let mut st = (&mut self.usage, &mut self.cfg.caches, vec![]);
            let resp = crate::dialog::Dialog::new("data-usage", "Data usage").width(620.0).show_with_footer(
                &ctx,
                &mut st,
                |ui, (usage, caches, asks)| *asks = crate::data_usage::dialog::show(ui, usage, caches),
                |ui, (usage, _, asks)| asks.extend(crate::data_usage::dialog::footer(ui, usage)),
            );
            let asks = st.2;
            for a in asks {
                if self.usage_ask(a) {
                    keep = false;
                }
            }
            if resp.should_close() {
                self.usage.confirm = None;
                keep = false;
            }
        }
        if let Modal::Calendars(d) = &mut modal {
            let mut st = (&mut **d, None);
            let calendar = &self.calendar;
            let resp = crate::dialog::Dialog::new("calendars", "Calendars").width(560.0).show_with_footer(
                &ctx,
                &mut st,
                |ui, (d, out)| *out = crate::calendar::dialog::show(ui, d, calendar),
                |ui, (d, out)| *out = crate::calendar::dialog::footer(ui, d).or(out.take()),
            );
            let out = st.1;
            if let Some(a) = out {
                if self.calendar_dialog_action(d, a) {
                    keep = false;
                }
            }
            if resp.should_close() {
                keep = false;
            }
        }
        if let Modal::MailAccounts(d) = &mut modal {
            let mut st = (&mut **d, None);
            let mail = &self.mail;
            let resp = crate::dialog::Dialog::new("mail-accounts", "Mail accounts").width(720.0).show_with_footer(
                &ctx,
                &mut st,
                |ui, (d, out)| *out = crate::mail::dialog::show(ui, d, mail),
                |ui, (d, out)| *out = crate::mail::dialog::footer(ui, d).or(out.take()),
            );
            let out = st.1;
            if let Some(a) = out {
                if self.mail_dialog_action(d, a) {
                    keep = false;
                }
            }
            if resp.should_close() {
                keep = false;
            }
            // closing the dialog (Done, Cancel, Esc, a click outside) stops its browser sign-ins (JOB-010, JOB-032)
            if !keep {
                self.mail.cancel_sign_ins();
            }
        }
        if let Modal::Social(d) = &mut modal {
            let accounts = self.cfg.social_accounts.clone();
            let check_minutes = self.cfg.social_check_minutes;
            let mut st = (&mut **d, None);
            let resp = crate::dialog::Dialog::new("social-accounts", "Feed accounts").width(560.0).show_with_footer(
                &ctx,
                &mut st,
                |ui, (d, out)| *out = crate::social::accounts_ui::show(ui, d, &accounts, check_minutes),
                |ui, (d, out)| *out = crate::social::accounts_ui::footer(ui, d).or(out.take()),
            );
            let close_asked = resp.should_close();
            if let Some(a) = st.1 {
                if self.social_dialog_action(a) {
                    keep = false;
                }
            }
            // Esc / clicking outside closes it, but not while a sign-in is running
            if close_asked && !d.busy() {
                keep = false;
            }
        }
        if let Modal::Vaults(m) = &mut modal {
            keep = self.vault_modal(&ctx, m);
        }
        if let Modal::TexSetup = &modal {
            keep = self.tex_setup_dialog(&ctx);
        }
        if let Modal::Import(d) = &mut modal {
            let folders = self.import_folders();
            let tags = self.tag_choices();
            let mut st = (&mut **d, None);
            let resp = crate::dialog::Dialog::new("import", "Import into the vault").width(620.0).show_with_footer(
                &ctx,
                &mut st,
                |ui, (d, out)| *out = crate::import::dialog::show(ui, d, &folders, &tags),
                |ui, (d, out)| *out = crate::import::dialog::footer(ui, d).or(out.take()),
            );
            let close_asked = resp.should_close();
            // the Word files are converted: go on with the import (IMP-035)
            let action = st.1.or_else(|| d.poll_conversion().then_some(crate::import::dialog::ImportAction::Import));
            if let Some(a) = action {
                if self.import_action(d, a) {
                    keep = false;
                }
            }
            if close_asked {
                keep = false;
            }
        }
        if let Modal::ImportSummary(s) = &modal {
            let mut asked = None;
            let resp = crate::dialog::Dialog::new("import-summary", "Import summary").width(560.0).show(&ctx, |ui| asked = crate::import::summary::show(ui, s));
            match asked {
                Some(crate::import::summary::SummaryAction::Open(rel)) => self.actions.push(Action::Open { rel, new_tab: true }),
                Some(crate::import::summary::SummaryAction::OpenAll) => {
                    self.actions.extend(s.notes.iter().map(|e| Action::Open { rel: e.rel.clone(), new_tab: true }));
                    keep = false;
                }
                Some(crate::import::summary::SummaryAction::Close) => keep = false,
                None => {}
            }
            if resp.should_close() {
                keep = false;
            }
        }
        if let Modal::Event(d) = &mut modal {
            let title = if d.original.is_some() { "Edit event" } else { "New event" };
            // the dialog, the calendars, and what the content or the footer ended with
            let mut st = (&mut **d, &mut self.calendar, None);
            let resp = crate::dialog::Dialog::new("event", title).width(460.0).show_with_footer(
                &ctx,
                &mut st,
                |ui, (d, store, out)| *out = crate::calendar::dialog::show_event(ui, d, store),
                |ui, (d, store, out)| {
                    // a finished save closes the dialog: its footer has nothing left to do
                    if out.is_none() {
                        *out = crate::calendar::dialog::event_footer(ui, d, store);
                    }
                },
            );
            let close_asked = resp.should_close();
            match st.2 {
                Some(crate::calendar::dialog::EventResult::Saved(msg)) => {
                    self.cal.last_event_calendar = Some(d.calendar.clone());
                    self.toast(msg);
                    keep = false;
                }
                Some(crate::calendar::dialog::EventResult::Cancel) => keep = false,
                None => {}
            }
            // Esc / clicking outside closes it, but not while a save is still running
            if close_asked && !d.is_saving() {
                keep = false;
            }
        }
        if let Modal::Export(d) = &mut modal {
            let title = match d.book.as_deref() {
                Some(b) if b.board => format!("Export the board “{}” to PDF", d.name),
                Some(_) => format!("Export the folder “{}” to PDF", d.name),
                None => format!("Export “{}” to PDF", d.name),
            };
            // a LaTeX export that is running or has just ended (TEX-016), or the book of a folder (BOOK-010)
            let finished = self.tex_export_tick(d, &ctx).or_else(|| self.book_export_tick(d)).or_else(|| self.note_export_tick(d));
            if d.tex.as_ref().is_some_and(|t| t.working()) || d.book.as_ref().is_some_and(|b| b.working()) || d.note_working() {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
            let mut st = (&mut **d, crate::pdf::export_dialog::DialogResult::Open);
            let resp = crate::dialog::Dialog::new("export-pdf", title).width(460.0).show_with_footer(&ctx, &mut st, |ui, (d, _)| crate::pdf::export_dialog::show(ui, d), |ui, (d, out)| *out = crate::pdf::export_dialog::footer(ui, d));
            let close_asked = resp.should_close();
            let result = st.1;
            let done = |s: &mut Self, d: &mut crate::pdf::export_dialog::ExportDialog, out: PathBuf| {
                s.toast(format!("Exported to {}", out.display()));
                if d.open_after {
                    if let Err(e) = crate::opener::open_exported_pdf(&out) {
                        s.toast(e);
                    }
                }
            };
            match finished {
                Some(Ok(out)) => {
                    done(self, d, out);
                    keep = false;
                }
                Some(Err(e)) => d.error = Some(e),
                None => {}
            }
            match result {
                // a note's or a folder's export through LaTeX (TEX-016, BOOK-013)
                crate::pdf::export_dialog::DialogResult::Export if d.latex() => d.error = self.start_tex_export(d, &ctx).err(),
                crate::pdf::export_dialog::DialogResult::Export if d.book.is_some() => d.error = self.start_book_export(d, &ctx).err(),
                crate::pdf::export_dialog::DialogResult::Cancel if d.book.as_ref().is_some_and(|b| b.working()) => {
                    // Cancel stops the export that is running, and a second one closes the dialog
                    if let Some(b) = d.book.as_mut() {
                        b.cancel();
                    }
                }
                // Cancel stops the export of a note that is running, and a second one closes the dialog (JOB-010)
                crate::pdf::export_dialog::DialogResult::Cancel if d.note_working() => d.cancel_note(),
                crate::pdf::export_dialog::DialogResult::Export => d.error = self.start_note_export(d, &ctx).err(),
                crate::pdf::export_dialog::DialogResult::Cancel => keep = false,
                crate::pdf::export_dialog::DialogResult::Open => {}
            }
            if d.tex.as_ref().is_some_and(|t| t.save_requested) {
                self.save_failed_tex(d);
            }
            // a hint's "Set up…", or the first LaTeX choice while something is missing (TEX-017)
            if let Some(step) = self.tex_setup_wanted(d) {
                let ret = if d.book.is_some() { crate::pdf::tex::setup::Return::ExportFolder(d.rel.clone()) } else { crate::pdf::tex::setup::Return::Export(d.rel.clone()) };
                self.open_tex_setup(Some(step), ret);
                keep = false;
            }
            if close_asked {
                keep = false;
            }
        }
        if let Modal::ExportDocx(d) = &mut modal {
            keep &= self.docx_export_modal(&ctx, d);
        }
        if keep && self.modal.is_none() {
            self.modal = Some(modal);
        }
    }
}

fn section_header(ui: &mut Ui, t: &str) {
    ui.label(egui::RichText::new(t).size(12.5).color(theme::palette().muted).strong());
    ui.add_space(2.0);
}

/// The file list of the left sidebar: the open folders and the keyboard selection.
#[derive(Default)]
pub struct FileTree {
    pub expanded: HashSet<PathBuf>,
    /// the selected row of the file list while it has the keyboard (Ctrl+H, Ctrl+Shift+B)
    pub sel: Option<PathBuf>,
    /// scroll the selected file list row into view on the next frame
    pub(crate) scroll: bool,
    /// the rows of the open folders, kept while nothing they come from changes (PERF-011)
    pub(crate) rows: TreeRows,
}

/// One row in the file tree. `open` is Some for folders.
/// A file or folder being dragged in the file list.
#[derive(Clone, Debug)]
pub struct TreeDrag(pub PathBuf);

/// The height of a row of the file list.
const TREE_ROW_H: f32 = 26.0;
/// How far above and below the view rows are drawn too, so a scroll step finds them ready.
const TREE_MARGIN: f32 = 150.0;

fn tree_row(ui: &mut Ui, depth: usize, name: &str, open: Option<bool>, active: bool, color: egui::Color32, badge: &str) -> egui::Response {
    let h = TREE_ROW_H;
    // click to open, drag to move into another folder
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click_and_drag());
    if active {
        ui.painter().rect_filled(r, CornerRadius::same(5), theme::palette().bg_active);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(5), theme::palette().bg_hover);
    }
    let x = r.min.x + 6.0 + depth as f32 * 16.0;
    // indent guides
    for d in 0..depth {
        let gx = r.min.x + 13.0 + d as f32 * 16.0;
        ui.painter().line_segment([Pos2::new(gx, r.min.y), Pos2::new(gx, r.max.y)], egui::Stroke::new(1.0, theme::palette().border));
    }
    let mut tx = x;
    if let Some(o) = open {
        let c = Pos2::new(x + 6.0, r.center().y);
        let pts = if o { vec![c + Vec2::new(-4.0, -2.0), c + Vec2::new(0.0, 2.5), c + Vec2::new(4.0, -2.0)] } else { vec![c + Vec2::new(-2.0, -4.0), c + Vec2::new(2.5, 0.0), c + Vec2::new(-2.0, 4.0)] };
        ui.painter().add(egui::Shape::line(pts, egui::Stroke::new(1.4, theme::palette().muted)));
        tx += 16.0;
    } else {
        tx += 4.0;
    }
    let fg = if active { theme::palette().text_strong } else { color };
    let galley = ui.painter().layout_no_wrap(name.to_string(), FontId::proportional(13.5), fg);
    let clip = Rect::from_min_max(Pos2::new(tx, r.min.y), Pos2::new(r.max.x - if badge.is_empty() { 4.0 } else { 60.0 }, r.max.y));
    ui.painter().with_clip_rect(clip).galley(Pos2::new(tx, r.center().y - galley.size().y / 2.0), galley, fg);
    if !badge.is_empty() {
        let br = Rect::from_min_max(Pos2::new(r.max.x - 58.0, r.center().y - 8.0), Pos2::new(r.max.x - 4.0, r.center().y + 8.0));
        ui.painter().rect_filled(br, CornerRadius::same(4), theme::palette().bg_hover);
        ui.painter().text(br.center(), egui::Align2::CENTER_CENTER, badge, FontId::proportional(10.5), theme::palette().muted);
    }
    let _ = paint_icon;
    resp
}

/// Order two tree entries. Times are (created, modified); ties fall back to the name.
pub(crate) fn sort_order(
    sort: crate::config::FileSort,
    name_a: &str,
    name_b: &str,
    ta: (Option<std::time::SystemTime>, Option<std::time::SystemTime>),
    tb: (Option<std::time::SystemTime>, Option<std::time::SystemTime>),
) -> std::cmp::Ordering {
    use crate::config::FileSort::*;
    let by_name = name_a.cmp(name_b);
    match sort {
        NameAsc => by_name,
        NameDesc => by_name.reverse(),
        CreatedAsc => ta.0.cmp(&tb.0).then(by_name),
        CreatedDesc => tb.0.cmp(&ta.0).then(by_name),
        ModifiedAsc => ta.1.cmp(&tb.1).then(by_name),
        ModifiedDesc => tb.1.cmp(&ta.1).then(by_name),
    }
}

/// Remove the due-date marker from a task's text (it is shown separately).
pub fn strip_due(text: &str) -> String {
    let mut s = text.to_string();
    for key in ["📅", "due:", "@due("] {
        if let Some(p) = s.find(key) {
            let after = s[p + key.len()..].trim_start();
            let skip = s.len() - after.len();
            let end = (skip + 10).min(s.len());
            if s.is_char_boundary(end) {
                let tail = s[end..].trim_start_matches(')').to_string();
                s = format!("{}{}", s[..p].trim_end(), if tail.is_empty() { String::new() } else { format!(" {}", tail.trim_start()) });
            }
        }
    }
    s
}

/// Today's date as YYYY-MM-DD (UTC), without a date library.
pub fn today() -> Option<String> {
    let secs = crate::calendar::time::now();
    let days = secs.div_euclid(86_400);
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

#[cfg(test)]
mod tests {
    // spec: NAV-003
    #[test]
    fn sorting_modes() {
        use crate::config::FileSort::*;
        use std::cmp::Ordering::*;
        use std::time::{Duration, UNIX_EPOCH};
        let old = (Some(UNIX_EPOCH + Duration::from_secs(10)), Some(UNIX_EPOCH + Duration::from_secs(100)));
        let new = (Some(UNIX_EPOCH + Duration::from_secs(20)), Some(UNIX_EPOCH + Duration::from_secs(50)));
        assert_eq!(super::sort_order(NameAsc, "a", "b", old, new), Less);
        assert_eq!(super::sort_order(NameDesc, "a", "b", old, new), Greater);
        assert_eq!(super::sort_order(CreatedAsc, "a", "b", old, new), Less);
        assert_eq!(super::sort_order(CreatedDesc, "a", "b", old, new), Greater);
        assert_eq!(super::sort_order(ModifiedAsc, "a", "b", old, new), Greater);
        assert_eq!(super::sort_order(ModifiedDesc, "a", "b", old, new), Less);
    }

    // spec: TASK-002
    #[test]
    fn due_is_stripped() {
        assert_eq!(super::strip_due("Write 📅 2026-12-01"), "Write");
        assert_eq!(super::strip_due("Pay due:2026-01-02 rent"), "Pay rent");
        assert_eq!(super::strip_due("plain"), "plain");
    }

    // spec: TASK-002
    #[test]
    fn today_looks_like_a_date() {
        let t = super::today().unwrap();
        assert_eq!(t.len(), 10);
        assert!(t.starts_with("20"));
    }
}

pub fn _views(_: View) {}

#[cfg(test)]
mod move_tests {
    use crate::app::{Options, ShaleApp};
    use std::path::{Path, PathBuf};

    // spec: VAULT-015, VAULT-042, VAULT-016
    #[test]
    fn moving_notes_and_folders_into_folders() {
        let d = std::env::temp_dir().join(format!("shale-move-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        crate::vault::write_sample_vault(&d).unwrap();
        std::fs::create_dir_all(d.join("Archiv")).unwrap();
        std::fs::create_dir_all(d.join("Projects/Sub")).unwrap();
        std::fs::write(d.join("Projects/Sub/Detail.md"), "back to [[Projects/Garden plan|the plan]]\n").unwrap();
        std::fs::write(d.join("Links.md"), "[[Projects/Garden plan]] · [[Garden plan]] · [Plan](Projects/Garden%20plan.md) · [[Projects/Sub/Detail]]\n").unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(d.clone()), view: None, keys: None }, false);
        app.vault.as_mut().unwrap().rescan();
        let read = |p: &str| std::fs::read_to_string(d.join(p)).unwrap();
        // a note into a folder, while it is open
        app.open(Path::new("Ideas.md"), false);
        assert_eq!(app.move_into(Path::new("Ideas.md"), Path::new("Archiv")).unwrap(), PathBuf::from("Archiv/Ideas.md"));
        assert!(d.join("Archiv/Ideas.md").exists() && !d.join("Ideas.md").exists());
        assert_eq!(app.current_path(), Some(PathBuf::from("Archiv/Ideas.md")), "the open tab follows");
        assert!(app.vault.as_ref().unwrap().exists("Ideas", None), "links by name keep working");
        // a whole folder into another one: path links to it and inside it follow
        let to = app.move_into(Path::new("Projects"), Path::new("Archiv")).unwrap();
        assert_eq!(to, PathBuf::from("Archiv/Projects"));
        assert!(d.join("Archiv/Projects/Sub/Detail.md").exists());
        assert_eq!(read("Links.md"), "[[Archiv/Projects/Garden plan]] · [[Garden plan]] · [Plan](Archiv/Projects/Garden%20plan.md) · [[Archiv/Projects/Sub/Detail]]\n");
        assert_eq!(read("Archiv/Projects/Sub/Detail.md"), "back to [[Archiv/Projects/Garden plan|the plan]]\n", "links inside the moved folder too");
        let v = app.vault.as_ref().unwrap();
        for n in v.notes.values() {
            for (l, r) in n.links.iter().zip(&n.resolved) {
                if l.target.contains("Garden plan") || l.target.contains("Detail") {
                    assert!(r.is_some(), "{} still resolves", l.target);
                }
            }
        }
        assert!(app.tree.expanded.contains(Path::new("Archiv")));
        // back to the top of the vault
        app.move_into(Path::new("Archiv/Ideas.md"), Path::new("")).unwrap();
        assert!(d.join("Ideas.md").exists());
        // refused: into itself, and a name that is taken
        assert!(app.move_into(Path::new("Archiv"), Path::new("Archiv/Projects")).unwrap_err().contains("itself"));
        std::fs::write(d.join("Archiv/Welcome.md"), "other\n").unwrap();
        app.vault.as_mut().unwrap().rescan();
        assert!(app.move_into(Path::new("Welcome.md"), Path::new("Archiv")).unwrap_err().contains("already has"));
        assert!(d.join("Welcome.md").exists(), "nothing moved");
        // into the folder it is in already: nothing happens
        assert_eq!(app.move_into(Path::new("Welcome.md"), Path::new("")).unwrap(), PathBuf::from("Welcome.md"));
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod tree_key_tests {
    use crate::app::{Action, LeftPanel, Options, ShaleApp};
    use eframe::egui::{self, Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2};
    use std::path::{Path, PathBuf};

    const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

    // spec: NAV-010, NAV-011, NAV-012
    #[test]
    fn the_file_list_is_browsed_with_j_k_ctrl_j_k_and_space() {
        let d = std::env::temp_dir().join(format!("shale-tree-keys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for f in ["A/a1.md", "A/a2.md", "B/b1.md", "r1.md", "r2.md"] {
            std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
            std::fs::write(d.join(f), "x\n").unwrap();
        }
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(d.clone()), view: None, keys: None }, false);
        app.side.left = LeftPanel::Files;
        app.open(Path::new("r1.md"), false);
        app.tree.expanded.clear(); // the start opened a first note (and its folder)
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut press = |app: &mut ShaleApp, k: Key, m: Modifiers| {
            t += 0.05;
            let mut events = vec![Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m }];
            if !m.ctrl && matches!(k, Key::J | Key::K) {
                events.push(Event::Text(format!("{k:?}").to_lowercase()));
            }
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 600.0))), time: Some(t), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.file_explorer(ui));
            });
            out.textures_delta.clear();
            app.tree.sel.clone()
        };
        let p = |s: &str| Some(PathBuf::from(s));
        // Ctrl+H / Ctrl+Shift+B: the file list gets the keyboard with the open note selected
        app.focus_sidebar(&ctx);
        assert_eq!(app.tree.sel, p("r1.md"));
        // rows: A, B (closed folders), r1, r2. j / k only stop on files
        assert_eq!(press(&mut app, Key::J, Modifiers::NONE), p("r2.md"));
        assert_eq!(press(&mut app, Key::J, Modifiers::NONE), p("r2.md"), "stops at the end");
        assert_eq!(press(&mut app, Key::K, Modifiers::NONE), p("r1.md"));
        assert_eq!(press(&mut app, Key::K, Modifiers::NONE), p("r1.md"), "no file above r1 (only folders)");
        // Ctrl+J / Ctrl+K only stop on folders
        assert_eq!(press(&mut app, Key::K, CTRL), p("B"));
        assert_eq!(press(&mut app, Key::K, CTRL), p("A"));
        assert_eq!(press(&mut app, Key::J, CTRL), p("B"));
        assert_eq!(press(&mut app, Key::K, CTRL), p("A"));
        // Space opens the folder; its files join the rows
        press(&mut app, Key::Space, Modifiers::NONE);
        assert!(app.tree.expanded.contains(Path::new("A")));
        assert_eq!(press(&mut app, Key::J, Modifiers::NONE), p("A/a1.md"));
        assert_eq!(press(&mut app, Key::J, Modifiers::NONE), p("A/a2.md"));
        assert_eq!(press(&mut app, Key::J, Modifiers::NONE), p("r1.md"), "B is a folder: skipped");
        assert_eq!(press(&mut app, Key::J, CTRL), p("r1.md"), "no folder below");
        assert_eq!(press(&mut app, Key::K, CTRL), p("B"));
        press(&mut app, Key::Space, Modifiers::NONE);
        assert_eq!(press(&mut app, Key::J, Modifiers::NONE), p("B/b1.md"));
        // Space again closes A (select it first); Enter on a file opens it, keyboard to the editor
        press(&mut app, Key::K, CTRL);
        press(&mut app, Key::K, CTRL);
        assert_eq!(app.tree.sel, p("A"));
        press(&mut app, Key::Space, Modifiers::NONE);
        assert!(!app.tree.expanded.contains(Path::new("A")));
        assert_eq!(press(&mut app, Key::J, Modifiers::NONE), p("B/b1.md"));
        app.actions.clear();
        assert_eq!(press(&mut app, Key::Enter, Modifiers::NONE), None, "Enter leaves the file list");
        assert!(matches!(app.actions.first(), Some(Action::Open { rel, .. }) if rel == Path::new("B/b1.md")));
        assert!(app.tabbar.tabs[app.tabbar.active].editor.request_focus, "…and the editor gets the keyboard");
        // Esc and Ctrl+L also give the keyboard back; a click elsewhere ends browsing
        app.focus_sidebar(&ctx);
        assert!(app.tree.sel.is_some());
        assert_eq!(press(&mut app, Key::Escape, Modifiers::NONE), None);
        app.focus_sidebar(&ctx);
        app.focus_editor();
        assert_eq!(app.tree.sel, None);
        app.focus_sidebar(&ctx);
        ctx.memory_mut(|m| m.request_focus(egui::Id::unique("somewhere else")));
        assert_eq!(press(&mut app, Key::J, Modifiers::NONE), None);
        // in the search panel, Ctrl+H means the search box
        app.side.left = LeftPanel::Search;
        app.focus_sidebar(&ctx);
        assert!(app.search.focus);
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod canvas_dialog_tests {
    use super::*;
    use egui::{Event, PointerButton, RawInput};

    /// One frame of the dialogs; returns every labelled widget on screen (from the accessibility tree).
    fn frame(ctx: &egui::Context, app: &mut ShaleApp, t: &mut f64, events: Vec<Event>) -> Vec<(String, Rect)> {
        *t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0))), time: Some(*t), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.modals(ui));
        });
        out.textures_delta.clear();
        let tree = out.platform_output.accesskit_update.expect("accessibility is on");
        tree.nodes.iter().filter_map(|(_, n)| Some((n.label()?.to_string(), n.bounds().map(|b| Rect::from_min_max(Pos2::new(b.x0 as f32, b.y0 as f32), Pos2::new(b.x1 as f32, b.y1 as f32)))?))).collect()
    }

    /// The widgets on screen once the dialog has settled (a new dialog is laid out unseen first,
    /// then measures its content and takes that size).
    fn look(ctx: &egui::Context, app: &mut ShaleApp, t: &mut f64) -> Vec<(String, Rect)> {
        for _ in 0..4 {
            frame(ctx, app, t, vec![]);
        }
        frame(ctx, app, t, vec![])
    }

    fn click(ctx: &egui::Context, app: &mut ShaleApp, t: &mut f64, label: &str) {
        let shown = look(ctx, app, t);
        let r = shown.iter().find(|(l, _)| l == label).unwrap_or_else(|| panic!("no {label:?} on screen: {:?}", shown.iter().map(|x| &x.0).collect::<Vec<_>>())).1;
        let p = r.center();
        let btn = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(ctx, app, t, vec![Event::PointerMoved(p)]);
        frame(ctx, app, t, vec![btn(true)]);
        frame(ctx, app, t, vec![btn(false)]);
    }

    // spec: DRAW-012, DRAW-013
    #[test]
    fn the_canvas_question_and_its_tick_box() {
        let (mut app, d) = crate::app::tests::app_with_canvas("canvas-dialog");
        let canvas = Path::new("Maps/Map.canvas");
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut t = 0.0;
        // Cancel opens nothing
        app.open(canvas, false);
        click(&ctx, &mut app, &mut t, "Cancel");
        assert!(app.modal.is_none() && app.actions.is_empty());
        assert_ne!(app.current_path().as_deref(), Some(canvas));
        // "Open read-only" opens it read-only
        app.open(canvas, false);
        let shown: Vec<String> = look(&ctx, &mut app, &mut t).into_iter().map(|x| x.0).collect();
        for want in ["Convert", "Open read-only", "Cancel", "Always convert canvases when opening them"] {
            assert!(shown.iter().any(|l| l == want), "{want:?} missing: {shown:?}");
        }
        click(&ctx, &mut app, &mut t, "Open read-only");
        assert!(app.modal.is_none());
        assert_eq!(app.current_path().as_deref(), Some(canvas));
        assert!(app.drawings[canvas].read_only.is_some());
        // ticking "always" greys out read-only; Convert then turns the setting on and converts
        app.drawings.remove(canvas);
        app.open(Path::new("Welcome.md"), false);
        app.open(canvas, false);
        click(&ctx, &mut app, &mut t, "Always convert canvases when opening them");
        assert!(matches!(&app.modal, Some(Modal::ConvertCanvas { always: true, .. })));
        click(&ctx, &mut app, &mut t, "Open read-only");
        assert!(app.modal.is_some(), "read-only can't be chosen while “always convert” is ticked");
        assert!(!app.cfg.auto_convert_canvas);
        click(&ctx, &mut app, &mut t, "Convert");
        assert!(app.modal.is_none() && app.cfg.auto_convert_canvas);
        assert!(matches!(app.actions.as_slice(), [Action::ConvertCanvas { rel, .. }] if rel == canvas));
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
#[path = "dialogs_ui_tests.rs"]
mod dialogs_ui_tests;
#[cfg(test)]
#[path = "panels_ui_tests.rs"]
mod ui_tests;
