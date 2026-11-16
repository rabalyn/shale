//! Sidebars (files, search, tags, linked notes, outline, local graph), the vault-wide
//! to-do list and the dialogs.

use crate::app::{graph_click, muted, Action, LeftPanel, Modal, RightPanel, SettingsTab, ShaleApp, View};
use crate::keys::{Action as KeyAction, Scope};
use crate::layout::{icon_button, paint_icon, Icon};
use crate::markdown;
use crate::theme;
use crate::vault::{rel_to_string, FileKind};
use eframe::egui::{self, CornerRadius, FontId, Key, Modifiers, Pos2, Rect, Sense, Ui, Vec2};
use std::path::{Path, PathBuf};

impl ShaleApp {
    pub fn left_sidebar(&mut self, ui: &mut Ui) {
        match self.left {
            LeftPanel::Files => self.file_explorer(ui),
            LeftPanel::Search => self.search_panel(ui),
            LeftPanel::Tags => self.tags_panel(ui),
        }
    }

    fn file_explorer(&mut self, ui: &mut Ui) {
        self.tree_keys(ui);
        // dropping anywhere on the file list (outside a folder) moves to the top of the vault
        let panel = ui.available_rect_before_wrap();
        self.tree_targets.push((panel, PathBuf::new(), false));
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
                self.expanded.clear();
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
        let vault_name = self.vault.as_ref().and_then(|v| v.root.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
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
            let root = PathBuf::new();
            self.tree(ui, &root, 0);
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
                if ui.add_enabled(self.can_restore_trashed(), egui::Button::new("Restore last deleted")).clicked() {
                    self.actions.push(Action::RestoreTrashed);
                    ui.close();
                }
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("▣  {vault_name}")).color(theme::TEXT).size(13.0)).on_hover_text(self.vault.as_ref().map(|v| v.root.to_string_lossy().into_owned()).unwrap_or_default());
        });
    }

    /// The files of a folder in the order the file list shows them (its sort setting).
    pub(crate) fn sorted_files_in(&self, dir: &Path) -> Vec<(PathBuf, String, FileKind)> {
        let Some(v) = &self.vault else { return vec![] };
        let sort = self.cfg.file_sort;
        let mut entries: Vec<&crate::vault::FileEntry> = v.files.iter().filter(|f| f.rel.parent() == Some(dir)).collect();
        entries.sort_by(|a, b| sort_order(sort, &a.display_name().to_lowercase(), &b.display_name().to_lowercase(), (a.created, a.modified), (b.created, b.modified)));
        entries.into_iter().map(|f| (f.rel.clone(), f.display_name(), f.kind)).collect()
    }

    /// The subfolders of a folder in the order the file list shows them.
    pub(crate) fn sorted_folders_in(&self, dir: &Path) -> Vec<PathBuf> {
        let Some(v) = &self.vault else { return vec![] };
        let sort = self.cfg.file_sort;
        let mut folders: Vec<PathBuf> = v.folders.iter().filter(|f| f.parent() == Some(dir)).cloned().collect();
        let fname = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        folders.sort_by(|a, b| {
            let (ta, tb) = (v.folder_times.get(a).copied().unwrap_or_default(), v.folder_times.get(b).copied().unwrap_or_default());
            sort_order(sort, &fname(a), &fname(b), ta, tb)
        });
        folders
    }

    /// The rows the file list shows right now, top to bottom: (path, is a folder).
    pub(crate) fn visible_tree_rows(&self) -> Vec<(PathBuf, bool)> {
        fn walk(app: &ShaleApp, dir: &Path, out: &mut Vec<(PathBuf, bool)>) {
            for f in app.sorted_folders_in(dir) {
                out.push((f.clone(), true));
                if app.expanded.contains(&f) {
                    walk(app, &f, out);
                }
            }
            out.extend(app.sorted_files_in(dir).into_iter().map(|(p, _, _)| (p, false)));
        }
        let mut out = Vec::new();
        walk(self, Path::new(""), &mut out);
        out
    }

    /// Keys while the file list has the keyboard: j / k files, Ctrl+J / Ctrl+K folders, Space
    /// opens or closes a folder, Enter opens, Esc back to the editor.
    fn tree_keys(&mut self, ui: &Ui) {
        let Some(sel) = self.tree_sel.clone() else { return };
        if ui.ctx().memory(|m| m.focused().is_some()) {
            // something else took the keyboard (a click into the editor, a dialog)
            self.tree_sel = None;
            return;
        }
        let rows = self.visible_tree_rows();
        let Some(cur) = rows.iter().position(|(r, _)| *r == sel).or(if rows.is_empty() { None } else { Some(0) }) else {
            self.tree_sel = None;
            return;
        };
        let (action, enter, esc) = ui.input_mut(|i| (self.keymap.poll(i, Scope::FileList), i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape)));
        let is = |k: KeyAction| action == Some(k);
        let (next, prev, next_dir, prev_dir, space) = (is(KeyAction::TreeNextFile), is(KeyAction::TreePrevFile), is(KeyAction::TreeNextFolder), is(KeyAction::TreePrevFolder), is(KeyAction::TreeToggleFolder));
        // the next row of the wanted kind in that direction (stays put at the ends)
        let step = |down: bool, dirs: bool| -> usize {
            let pick = |i: &usize| rows[*i].1 == dirs;
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
        let (path, is_dir) = rows[new].clone();
        self.tree_scroll |= new != cur;
        self.tree_sel = Some(path.clone());
        if (space || enter) && is_dir {
            if !self.expanded.remove(&path) {
                self.expanded.insert(path);
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
        if self.tree_sel.as_deref() == Some(rel) {
            ui.painter().rect_stroke(resp.rect, CornerRadius::same(4), egui::Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
            if std::mem::take(&mut self.tree_scroll) {
                resp.scroll_to_me(Some(egui::Align::Center));
            }
        }
    }

    fn tree(&mut self, ui: &mut Ui, dir: &Path, depth: usize) {
        let folders = self.sorted_folders_in(dir);
        let files = self.sorted_files_in(dir);
        let current = self.current_path();
        for f in folders {
            let open = self.expanded.contains(&f);
            let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let resp = tree_row(ui, depth, &name, Some(open), false, theme::TEXT, "");
            self.mark_tree_sel(ui, &f, &resp);
            self.add_drop_zone(resp.rect.intersect(ui.clip_rect()), f.clone(), None);
            self.tree_targets.push((resp.rect.intersect(ui.clip_rect()), f.clone(), !open));
            if resp.drag_started() {
                egui::DragAndDrop::set_payload(ui.ctx(), TreeDrag(f.clone()));
            }
            if resp.clicked() {
                if open {
                    self.expanded.remove(&f);
                } else {
                    self.expanded.insert(f.clone());
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
                if ui.button("Rename…").clicked() {
                    self.actions.push(Action::Rename(f.clone()));
                    ui.close();
                }
                if ui.button("Move to trash").clicked() {
                    self.actions.push(Action::Trash(f.clone()));
                    ui.close();
                }
            });
            if open {
                self.tree(ui, &f, depth + 1);
            }
        }
        for (rel, name, kind) in files {
            let active = current.as_deref() == Some(rel.as_path());
            let badge = match kind {
                FileKind::Drawing if crate::drawing::canvas::is_canvas_path(&rel) => "canvas",
                FileKind::Drawing => "drawing",
                FileKind::Other => rel.extension().and_then(|e| e.to_str()).unwrap_or(""),
                FileKind::Note => "",
            };
            let color = if kind == FileKind::Other { theme::MUTED } else { theme::TEXT };
            let resp = tree_row(ui, depth, &name, None, active, color, badge);
            self.mark_tree_sel(ui, &rel, &resp);
            // dropping on a file puts the dragged thing next to it
            self.tree_targets.push((resp.rect.intersect(ui.clip_rect()), rel.parent().map(|p| p.to_path_buf()).unwrap_or_default(), false));
            if resp.drag_started() {
                egui::DragAndDrop::set_payload(ui.ctx(), TreeDrag(rel.clone()));
            }
            self.add_drop_zone(resp.rect.intersect(ui.clip_rect()), rel.parent().map(|p| p.to_path_buf()).unwrap_or_default(), (kind == FileKind::Note).then(|| rel.clone()));
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

    pub(crate) fn search_panel(&mut self, ui: &mut Ui) {
        ui.label(egui::RichText::new("Search").size(13.0).color(theme::MUTED));
        let id = egui::Id::unique("search-input");
        if std::mem::take(&mut self.focus_search) {
            // Ctrl+Shift+F: always into the box, with the old search selected (type to replace it,
            // or press an arrow key to change it)
            let mut st = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
            let n = self.search.chars().count();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
            st.store(ui.ctx(), id);
            ui.ctx().memory_mut(|m| m.request_focus(id));
        }
        // ↓ in the box goes into the result list (checked before the box sees the key)
        let box_focused = ui.ctx().memory(|m| m.has_focus(id));
        let down_from_box = box_focused && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown));
        let r = ui.add(egui::TextEdit::singleline(&mut self.search).id(id).hint_text("Search all notes…").desired_width(f32::INFINITY));
        if r.changed() || r.has_focus() {
            self.search_sel = None;
        }
        let enter_from_box = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if self.search.is_empty() && !r.has_focus() && self.search_sel.is_none() {
            r.request_focus();
        }
        ui.add_space(6.0);
        let Some(v) = &self.vault else { return };
        let results = v.search(&self.search);
        // the stops the keyboard moves between: (result, found line or None for a name-only hit)
        let stops: Vec<(usize, Option<usize>)> = results.iter().enumerate().flat_map(|(fi, (_, lines))| if lines.is_empty() { vec![(fi, None)] } else { (0..lines.len()).map(|li| (fi, Some(li))).collect() }).collect();
        let entering = (down_from_box || enter_from_box) && !stops.is_empty();
        if entering {
            self.search_sel = Some(0);
            self.search_scroll = true;
            ui.ctx().memory_mut(|m| m.surrender_focus(id));
        }
        // browsing: only while no text field has the keyboard (so j and k never type)
        let mut open_sel = false;
        // (not in the frame the list was entered: the Enter that got us here must not open it)
        if let Some(sel) = self.search_sel.filter(|_| !entering) {
            let free = ui.ctx().memory(|m| m.focused().is_none());
            if !free || stops.is_empty() {
                self.search_sel = None;
            } else {
                let sel = sel.min(stops.len() - 1);
                let (action, enter, back) = ui.input_mut(|i| (self.keymap.poll(i, Scope::SearchResults), i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape) | i.consume_key(Modifiers::NONE, Key::Slash)));
                let is = |k: KeyAction| action == Some(k);
                let (next, prev, next_file, prev_file) = (is(KeyAction::HitNext), is(KeyAction::HitPrev), is(KeyAction::HitNextFile), is(KeyAction::HitPrevFile));
                let file = stops[sel].0;
                let new = if next_file {
                    stops.iter().position(|s| s.0 > file).unwrap_or(sel)
                } else if prev_file {
                    match file.checked_sub(1) {
                        Some(f) => stops.iter().position(|s| s.0 == f).unwrap_or(sel),
                        None => sel,
                    }
                } else if next {
                    (sel + 1).min(stops.len() - 1)
                } else if prev {
                    sel.saturating_sub(1)
                } else {
                    sel
                };
                self.search_sel = Some(new);
                self.search_scroll |= new != sel;
                if back {
                    self.search_sel = None;
                    ui.ctx().memory_mut(|m| m.request_focus(id));
                } else if enter {
                    open_sel = true;
                }
            }
        }
        if !self.search.trim().is_empty() {
            let hint = if self.search_sel.is_some() {
                "  ·  j/k, Ctrl+J/K next file, Enter opens, Esc back"
            } else if stops.is_empty() {
                ""
            } else {
                "  ·  ↓ to browse"
            };
            ui.label(muted(format!("{} result{}{hint}", results.len(), if results.len() == 1 { "" } else { "s" })).size(12.0));
        }
        let selected = self.search_sel.map(|i| stops[i]);
        if open_sel {
            if let Some((fi, li)) = selected {
                let rel = results[fi].0.clone();
                self.actions.push(match li {
                    Some(li) => {
                        let (line, col, _) = results[fi].1[li];
                        Action::GotoMatch { rel, line, col }
                    }
                    None => Action::Open { rel, new_tab: false },
                });
            }
            self.search_sel = None;
        }
        let scroll = std::mem::take(&mut self.search_scroll);
        let posts = if self.search.trim().is_empty() { vec![] } else { self.social.index.search(&self.search, 20) };
        let mark = |ui: &Ui, rect: egui::Rect| {
            ui.painter().rect_stroke(rect.expand(1.0), CornerRadius::same(4), egui::Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Outside);
        };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (fi, (rel, lines)) in results.iter().enumerate() {
                let name = v.file(rel).map(|f| f.display_name()).unwrap_or_default();
                let head = ui.add(egui::Label::new(egui::RichText::new(&name).color(theme::TEXT_STRONG)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
                if head.clicked() {
                    self.actions.push(Action::Open { rel: rel.clone(), new_tab: false });
                }
                if selected == Some((fi, None)) {
                    mark(ui, head.rect);
                    if scroll {
                        head.scroll_to_me(Some(egui::Align::Center));
                    }
                }
                for (li, (line, col, text)) in lines.iter().enumerate() {
                    let shown: String = text.chars().take(120).collect();
                    let resp = egui::Frame::new().fill(theme::BG_PRIMARY).corner_radius(CornerRadius::same(4)).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(egui::RichText::new(shown).size(12.5).color(theme::MUTED)).wrap())
                    });
                    if selected == Some((fi, Some(li))) {
                        mark(ui, resp.response.rect);
                        if scroll {
                            resp.response.scroll_to_me(Some(egui::Align::Center));
                        }
                    }
                    if resp.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        self.actions.push(Action::GotoMatch { rel: rel.clone(), line: *line, col: *col });
                    }
                }
                ui.add_space(6.0);
            }
            // posts of the feeds (SOC-036): a click opens the post in its feed
            if !posts.is_empty() {
                ui.label(muted(format!("Posts  {}", posts.len())).size(12.0));
            }
            let now = crate::calendar::time::now();
            self.social.hit_rects.clear();
            for (i, h) in posts.iter().enumerate() {
                let resp = egui::Frame::new().fill(theme::BG_PRIMARY).corner_radius(CornerRadius::same(4)).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let who = format!("{} · @{} · {}", h.author_name, h.author_handle, crate::social::feed_ui::age_text(h.created_at, now));
                    ui.add(egui::Label::new(egui::RichText::new(who).size(12.5).color(theme::TEXT_STRONG)).truncate());
                    ui.add(egui::Label::new(egui::RichText::new(&h.snippet).size(12.5).color(theme::MUTED)).wrap());
                });
                self.social.hit_rects.push((i, resp.response.rect));
                if resp.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    self.actions.push(Action::OpenPost(Box::new(h.clone())));
                }
                ui.add_space(4.0);
            }
        });
    }

    fn tags_panel(&mut self, ui: &mut Ui) {
        ui.label(egui::RichText::new("Tags").size(13.0).color(theme::MUTED));
        ui.add_space(4.0);
        let Some(v) = &self.vault else { return };
        let tags = v.all_tags();
        if tags.is_empty() {
            ui.label(muted("No #tags in this vault yet."));
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (tag, n) in tags {
                ui.horizontal(|ui| {
                    if ui.add(egui::Label::new(egui::RichText::new(format!("#{tag}")).color(theme::TAG)).sense(Sense::click())).on_hover_text("Search for this tag").clicked() {
                        self.search = format!("#{tag}");
                        self.left = LeftPanel::Search;
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
            self.add_calendar_area(area);
            egui::ScrollArea::vertical().id_salt("side-calendar").auto_shrink([false, false]).show(ui, |ui| {
                act = self.cal_side.show(ui, &self.calendar, monday, false);
            });
            if let Some(a) = act {
                self.calendar_action(a);
            }
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
                let bl = v.backlinks(&rel);
                let um = v.unlinked_mentions(&rel);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    section_header(ui, &format!("Linked mentions  {}", bl.len()));
                    if bl.is_empty() {
                        ui.label(muted(format!("No notes link to “{name}” yet.")).size(12.5));
                    }
                    for (src, lines) in &bl {
                        self.mention_block(ui, src, lines);
                    }
                    ui.add_space(10.0);
                    section_header(ui, &format!("Unlinked mentions  {}", um.len()));
                    for (src, lines) in &um {
                        self.mention_block(ui, src, lines);
                    }
                });
            }
            RightPanel::Outgoing => {
                let out = v.outgoing(&rel);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let (ok, missing): (Vec<_>, Vec<_>) = out.into_iter().partition(|o| o.2.is_some());
                    section_header(ui, &format!("Links  {}", ok.len()));
                    for (display, _target, r) in ok {
                        if let Some(r) = r {
                            let resp = ui.add(egui::Label::new(egui::RichText::new(display).color(theme::LINK)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
                            if resp.clicked() {
                                self.actions.push(Action::Open { rel: r, new_tab: false });
                            }
                        }
                    }
                    ui.add_space(10.0);
                    section_header(ui, &format!("Not created yet  {}", missing.len()));
                    for (display, target, _) in missing {
                        // a path link whose name exists in other places: offer them (LINK-012)
                        let candidates = v.fix_candidates(&target, Some(&rel));
                        ui.horizontal_wrapped(|ui| {
                            let resp = ui.add(egui::Label::new(egui::RichText::new(display).color(theme::LINK_MISSING)).sense(Sense::click())).on_hover_text("Click to create this note").on_hover_cursor(egui::CursorIcon::PointingHand);
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
                let heads = markdown::find_headings(&text);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if heads.is_empty() {
                        ui.label(muted("No headings in this note."));
                    }
                    let min = heads.iter().map(|h| h.1).min().unwrap_or(1);
                    for (line, level, title) in heads {
                        ui.horizontal(|ui| {
                            ui.add_space((level - min) as f32 * 14.0);
                            if ui.add(egui::Label::new(egui::RichText::new(title).color(theme::TEXT)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                self.actions.push(Action::GotoLine { rel: rel.clone(), line });
                            }
                        });
                    }
                });
            }
            RightPanel::Calendar => {}
            RightPanel::LocalGraph => {
                ui.horizontal(|ui| {
                    ui.label(muted("Depth"));
                    let mut d = self.local_graph.depth as i32;
                    if ui.add(egui::Slider::new(&mut d, 1..=3)).changed() {
                        self.local_graph.depth = d as usize;
                        self.local_graph.focus = None;
                    }
                });
                if self.local_graph.focus.as_deref() != Some(rel.as_path()) {
                    self.local_graph.focus = Some(rel.clone());
                }
                if let Some(c) = self.local_graph.ui(ui, v, Some(&rel), true) {
                    graph_click(&mut self.actions, c);
                }
            }
        }
    }

    fn mention_block(&mut self, ui: &mut Ui, src: &Path, lines: &[(usize, String)]) {
        let Some(v) = &self.vault else { return };
        let name = v.file(src).map(|f| f.display_name()).unwrap_or_default();
        if ui.add(egui::Label::new(egui::RichText::new(&name).color(theme::TEXT_STRONG)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.actions.push(Action::Open { rel: src.to_path_buf(), new_tab: false });
        }
        for (line, text) in lines.iter().take(5) {
            let shown: String = text.chars().take(160).collect();
            let resp = egui::Frame::new().fill(theme::BG_PRIMARY).corner_radius(CornerRadius::same(4)).inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(egui::RichText::new(shown).size(12.5).color(theme::MUTED)).wrap());
            });
            if resp.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Go to this line").clicked() {
                self.actions.push(Action::GotoLine { rel: src.to_path_buf(), line: *line });
            }
        }
        ui.add_space(6.0);
    }

    pub fn tasks_view(&mut self, ui: &mut Ui) {
        let Some(v) = &self.vault else { return };
        let all = v.all_tasks();
        let open = all.iter().filter(|(_, t)| !t.done).count();
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
                    ui.label(egui::RichText::new("Tasks").size(28.0).color(theme::TEXT_STRONG));
                    ui.label(muted(format!("{open} open · {done} done · from every note in the vault")));
                    ui.add_space(10.0);
                    // progress bar
                    let frac = if all.is_empty() { 0.0 } else { done as f32 / all.len() as f32 };
                    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
                    ui.painter().rect_filled(r, CornerRadius::same(3), theme::BG_HOVER);
                    ui.painter().rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width() * frac, r.height())), CornerRadius::same(3), theme::ACCENT);
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.tasks.query).hint_text("Filter tasks or #tags…").desired_width(260.0));
                        ui.checkbox(&mut self.tasks.show_done, "Show done");
                        ui.checkbox(&mut self.tasks.only_due, "Only with due date");
                    });
                    ui.add_space(10.0);
                    let q = self.tasks.query.to_lowercase();
                    let mut shown = 0;
                    let mut current_note: Option<PathBuf> = None;
                    let mut list: Vec<_> = all
                        .iter()
                        .filter(|(_, t)| self.tasks.show_done || !t.done)
                        .filter(|(_, t)| !self.tasks.only_due || t.due.is_some())
                        .filter(|(p, t)| q.is_empty() || t.text.to_lowercase().contains(&q) || rel_to_string(p).to_lowercase().contains(&q))
                        .collect();
                    if self.tasks.only_due {
                        list.sort_by(|a, b| a.1.due.cmp(&b.1.due));
                    }
                    for (rel, t) in list {
                        if !self.tasks.only_due && current_note.as_ref() != Some(rel) {
                            current_note = Some(rel.clone());
                            ui.add_space(8.0);
                            let name = v.file(rel).map(|f| f.display_name()).unwrap_or_default();
                            let folder = rel.parent().map(rel_to_string).filter(|s| !s.is_empty());
                            ui.horizontal(|ui| {
                                if ui.add(egui::Label::new(egui::RichText::new(&name).size(15.0).color(theme::TEXT_STRONG)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    self.actions.push(Action::Open { rel: rel.clone(), new_tab: false });
                                }
                                if let Some(f) = folder {
                                    ui.label(muted(f).size(12.0));
                                }
                            });
                        }
                        shown += 1;
                        ui.horizontal(|ui| {
                            ui.add_space(t.indent as f32 * 8.0);
                            let mut d = t.done;
                            if ui.checkbox(&mut d, "").on_hover_text("Tick off (saved to the note)").clicked() {
                                self.actions.push(Action::ToggleTask { rel: rel.clone(), offset: t.check_offset });
                            }
                            let shown = strip_due(&t.text);
                            let (link, plain) = crate::reader::inline_line(ui, &shown, v, rel, 14.0, t.done);
                            if let Some(a) = link {
                                crate::app::handle_reader_action(&mut self.actions, a, rel);
                            } else if plain {
                                self.actions.push(Action::GotoLine { rel: rel.clone(), line: t.line });
                            }
                            if let Some(due) = &t.due {
                                let overdue = today().map(|d| due.as_str() < d.as_str()).unwrap_or(false) && !t.done;
                                ui.label(egui::RichText::new(format!("due {due}")).size(12.0).color(if overdue { theme::LINK_MISSING } else { theme::ACCENT }));
                            }
                            if self.tasks.only_due {
                                ui.label(muted(v.file(rel).map(|f| f.display_name()).unwrap_or_default()).size(12.0));
                            }
                        });
                    }
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
        let ctx = ui.ctx().clone();
        match &mut modal {
            Modal::Picker(p) => {
                if let Some(v) = &self.vault {
                    let data = crate::search_all::Ctx { vault: v, mail: self.mail.index(), posts: &self.social.index };
                    let out = crate::picker::show(&ctx, p, &data);
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
                    let create = !query.trim().is_empty() && !items.iter().any(|(_, _, n)| n.eq_ignore_ascii_case(query.trim()));
                    let n = items.len() + create as usize;
                    let keys = crate::picker::ListKeys::take(ui);
                    keys.select(sel, n);
                    ui.add_space(6.0);
                    let mut pick: Option<usize> = if keys.enter && n > 0 { Some(*sel) } else { None };
                    for (i, (_, rel, name)) in items.iter().enumerate() {
                        let folder = rel.parent().map(rel_to_string).unwrap_or_default();
                        let label = if folder.is_empty() { name.clone() } else { format!("{name}    {folder}") };
                        if ui.selectable_label(i == *sel, label).clicked() {
                            pick = Some(i);
                        }
                    }
                    if create && ui.selectable_label(*sel == items.len(), egui::RichText::new(format!("＋ Create “{}”", query.trim())).color(theme::LINK_MISSING)).clicked() {
                        pick = Some(items.len());
                    }
                    ui.label(muted("↑↓ to move · Enter to open · Esc to close").size(11.5));
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
                        ui.label(muted("Links in other notes will be updated.").size(12.0));
                    }
                    if let Some(e) = error {
                        ui.colored_label(theme::LINK_MISSING, e.as_str());
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
                                        self.expanded.insert(dir.to_path_buf());
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
                    ui.label(format!("“{}” will be moved to the .trash folder inside your vault. You can get it back from there.", rel_to_string(rel)));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("Move to trash").color(theme::LINK_MISSING)).clicked() {
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
            Modal::Export(_) | Modal::Calendars(_) | Modal::Event(_) | Modal::Import(_) | Modal::MailAccounts(_) | Modal::Social(_) | Modal::DataUsage => {}
            Modal::ConvertCanvas { rel, new_tab, always, existing } => {
                let resp = crate::dialog::Dialog::new("convert-canvas", "Convert to an editable drawing?").width(460.0).show(&ctx, |ui| {
                    let name = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    ui.label(format!("“{name}” is an Obsidian Canvas. Shale shows canvases read-only."));
                    match existing.as_ref() {
                        Some(copy) => ui.label(format!("It was converted before: “{}”.", rel_to_string(copy))),
                        None => ui.label(format!("Converting makes the drawing “{}” next to it (Obsidian opens it with the Excalidraw plugin). The canvas file is not changed.", crate::drawing::canvas::drawing_name_for(rel))),
                    };
                    ui.add_space(6.0);
                    ui.checkbox(always, "Always convert canvases when opening them").on_hover_text("Also in Settings → Drawings");
                    ui.add_space(6.0);
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
            Modal::Settings { vault_path, tab, keys } => {
                let mut done = false;
                let body = |ui: &mut egui::Ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(tab, SettingsTab::General, "General");
                        ui.selectable_value(tab, SettingsTab::Keybindings, "Keybindings");
                    });
                    ui.separator();
                    if *tab == SettingsTab::Keybindings {
                        if crate::keys_ui::show(ui, keys, &mut self.keymap) {
                            self.cfg.keybindings = self.keymap.overrides();
                            self.save_config();
                        }
                        return;
                    }
                    self.keymap.recording = false;
                    ui.label(egui::RichText::new("Vault folder").color(theme::MUTED));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(vault_path).desired_width(360.0));
                        if ui.button("Open").clicked() {
                            let p = PathBuf::from(crate::layout::shellexpand(vault_path));
                            if p.is_dir() {
                                self.actions.push(Action::OpenVault(p));
                                keep = false;
                            } else {
                                self.toasts.push(("That folder doesn't exist".into(), std::time::Instant::now()));
                            }
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Calendars").color(theme::MUTED));
                    ui.horizontal(|ui| {
                        if ui.button("Calendars…").on_hover_text("Add local calendars or CalDAV accounts").clicked() {
                            open_calendars = true;
                        }
                        ui.label(muted(format!("{} configured", self.cfg.calendars.len())).size(12.0));
                    });
                    ui.add_space(8.0);
                    if ui.button("Close this vault (back to the start screen)").clicked() {
                        self.flush_all();
                        self.remember_open_files();
                        self.save_config();
                        self.vdata = crate::vault_state::VaultData::default();
                        self.vault = None;
                        self.tabs.clear();
                        self.cfg.vault = None;
                        keep = false;
                    }
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Links").color(theme::MUTED));
                    let mut changed = false;
                    changed |= ui.checkbox(&mut self.cfg.ctrl_to_follow, "Follow links only with Ctrl+click (plain click places the cursor)").changed();
                    changed |= ui.checkbox(&mut self.cfg.new_notes_next_to_current, "Create notes from red links next to the current note (instead of the vault root)").changed();
                    changed |= ui.checkbox(&mut self.cfg.update_links_on_rename, "Update links in other notes when renaming").changed();
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("People").color(theme::MUTED));
                    ui.horizontal(|ui| {
                        ui.label("People folder");
                        changed |= ui
                            .add(egui::TextEdit::singleline(&mut self.vdata.settings.people_dir).hint_text(crate::people::DEFAULT_DIR).desired_width(220.0))
                            .on_hover_text("Contact notes live here. Type [[p and a name to link a person; imported e-mails and events find their people here.")
                            .changed();
                    });
                    ui.label(muted("Type [[p followed by a name to pick a person from this folder (or make a new contact note).").size(12.0));
                    ui.add_space(8.0);
                    changed |= self.sync_settings_ui(ui);
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Drawings").color(theme::MUTED));
                    changed |= ui
                        .checkbox(&mut self.cfg.auto_convert_canvas, "Convert Obsidian canvases to editable drawings when opening them (instead of asking)")
                        .on_hover_text("Off: Shale asks, and can show the canvas read-only. The canvas file is never changed.")
                        .changed();
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Appearance").color(theme::MUTED));
                    changed |= ui.checkbox(&mut self.cfg.readable_width, "Readable line length").changed();
                    changed |= ui.checkbox(&mut self.cfg.dark_canvas, "Dark drawing canvas").changed();
                    changed |= ui.checkbox(&mut self.cfg.inline_images, "Show embedded images inside the editor (the link text appears while the cursor is on it)").changed();
                    changed |= ui.checkbox(&mut self.cfg.vim_mode, "Vim-style editing: notes open in normal mode (h j k l w b, :line, /search, v / V, d y p); i a o to type, Esc back").changed();
                    ui.horizontal(|ui| {
                        ui.label("Text size");
                        changed |= ui.add(egui::Slider::new(&mut self.cfg.font_size, 12.0..=24.0)).changed();
                    });
                    ui.add_space(8.0);
                    self.post_index_settings(ui);
                    ui.label(egui::RichText::new("Data and caches").color(theme::MUTED));
                    ui.horizontal(|ui| {
                        if ui.button("Data usage…").on_hover_text("What Shale keeps on disk, and its caches").clicked() {
                            open_usage = true;
                        }
                        let total = self.usage.total().map(|t| format!("caches: {}", crate::data_usage::human_bytes(t))).unwrap_or_else(|| "caches: not counted yet".into());
                        ui.label(muted(total).size(12.0));
                    });
                    egui::CollapsingHeader::new("How long caches are kept").id_salt("settings-retention").show(ui, |ui| {
                        for id in crate::data_usage::CacheId::ALL.into_iter().filter(|id| !id.kept_current()) {
                            ui.label(id.name());
                            ui.indent(("retention", id), |ui| {
                                if crate::data_usage::dialog::retention_ui(ui, id, &mut self.cfg.caches) {
                                    self.usage.setting_changed(id);
                                    changed = true;
                                }
                            });
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Troubleshooting").color(theme::MUTED));
                    if ui.checkbox(&mut self.cfg.input_log, "Record keyboard input to a file (to find keyboard problems)").changed() {
                        if self.cfg.input_log {
                            self.start_input_log();
                        }
                        changed = true;
                    }
                    if self.cfg.input_log {
                        let p = crate::config::input_log_path();
                        ui.horizontal(|ui| {
                            ui.label(muted(p.to_string_lossy()).size(12.0));
                            if ui.small_button("Copy path").clicked() {
                                ui.ctx().copy_text(p.to_string_lossy().into_owned());
                            }
                        });
                    }
                    ui.label(muted(format!("Build {}", crate::config::build_stamp())).size(12.0));
                    if changed {
                        self.save_config();
                    }
                };
                let resp = crate::dialog::Dialog::new("settings", "Settings").width(640.0).show_with_footer(&ctx, body, |ui| {
                    if ui.button("Done").clicked() {
                        done = true;
                    }
                });
                if done {
                    keep = false;
                }
                if resp.should_close() {
                    self.keymap.recording = false;
                    keep = false;
                }
            }
        }
        if open_calendars {
            self.open_calendar_settings();
            keep = false;
        }
        if open_usage {
            self.open_data_usage();
            keep = false;
        }
        if let Modal::DataUsage = &modal {
            let mut asks = vec![];
            let resp = crate::dialog::Dialog::new("data-usage", "Data usage").width(620.0).show(&ctx, |ui| {
                asks = crate::data_usage::dialog::show(ui, &mut self.usage, &mut self.cfg.caches);
            });
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
            let mut out = None;
            let resp = crate::dialog::Dialog::new("calendars", "Calendars").width(560.0).show(&ctx, |ui| {
                out = crate::calendar::dialog::show(ui, d, &self.calendar);
            });
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
            let mut out = None;
            let mail = &self.mail;
            let resp = crate::dialog::Dialog::new("mail-accounts", "Mail accounts").width(720.0).show(&ctx, |ui| {
                out = crate::mail_dialog::show(ui, d, mail);
            });
            if let Some(a) = out {
                if self.mail_dialog_action(d, a) {
                    keep = false;
                }
            }
            if resp.should_close() {
                keep = false;
            }
        }
        if let Modal::Social(d) = &mut modal {
            let accounts = self.cfg.social_accounts.clone();
            let check_minutes = self.cfg.social_check_minutes;
            let resp = crate::dialog::Dialog::new("social-accounts", "Feed accounts").width(560.0).show(&ctx, |ui| crate::social::accounts_ui::show(ui, d, &accounts, check_minutes));
            let close_asked = resp.should_close();
            if let Some(a) = resp.inner.flatten() {
                if self.social_dialog_action(a) {
                    keep = false;
                }
            }
            // Esc / clicking outside closes it, but not while a sign-in is running
            if close_asked && !d.busy() {
                keep = false;
            }
        }
        if let Modal::Import(d) = &mut modal {
            let folders = self.import_folders();
            let tags = self.tag_choices();
            let resp = crate::dialog::Dialog::new("import", "Import into the vault").width(620.0).show(&ctx, |ui| crate::import_dialog::show(ui, d, &folders, &tags));
            let close_asked = resp.should_close();
            if let Some(a) = resp.inner.flatten() {
                if self.import_action(d, a) {
                    keep = false;
                }
            }
            if close_asked {
                keep = false;
            }
        }
        if let Modal::Event(d) = &mut modal {
            let store = &mut self.calendar;
            let title = if d.original.is_some() { "Edit event" } else { "New event" };
            let resp = crate::dialog::Dialog::new("event", title).width(460.0).show(&ctx, |ui| crate::calendar::dialog::show_event(ui, d, store));
            let close_asked = resp.should_close();
            match resp.inner.flatten() {
                Some(crate::calendar::dialog::EventResult::Saved(msg)) => {
                    self.last_event_calendar = Some(d.calendar.clone());
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
            let title = format!("Export “{}” to PDF", d.name);
            let resp = crate::dialog::Dialog::new("export-pdf", title).width(460.0).show(&ctx, |ui| crate::export_dialog::show(ui, d));
            let close_asked = resp.should_close();
            match resp.inner.unwrap_or(crate::export_dialog::DialogResult::Open) {
                crate::export_dialog::DialogResult::Export => match self.run_export(d) {
                    Ok(out) => {
                        self.toast(format!("Exported to {}", out.display()));
                        if d.open_after {
                            if let Err(e) = crate::opener::open_exported_pdf(&out) {
                                self.toast(e);
                            }
                        }
                        keep = false;
                    }
                    Err(e) => d.error = Some(e),
                },
                crate::export_dialog::DialogResult::Cancel => keep = false,
                crate::export_dialog::DialogResult::Open => {}
            }
            if close_asked {
                keep = false;
            }
        }
        if keep && self.modal.is_none() {
            self.modal = Some(modal);
        }
    }
}

fn section_header(ui: &mut Ui, t: &str) {
    ui.label(egui::RichText::new(t).size(12.5).color(theme::MUTED).strong());
    ui.add_space(2.0);
}

/// One row in the file tree. `open` is Some for folders.
/// A file or folder being dragged in the file list.
#[derive(Clone, Debug)]
pub struct TreeDrag(pub PathBuf);

fn tree_row(ui: &mut Ui, depth: usize, name: &str, open: Option<bool>, active: bool, color: egui::Color32, badge: &str) -> egui::Response {
    let h = 26.0;
    // click to open, drag to move into another folder
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click_and_drag());
    if active {
        ui.painter().rect_filled(r, CornerRadius::same(5), theme::BG_ACTIVE);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(5), theme::BG_HOVER);
    }
    let x = r.min.x + 6.0 + depth as f32 * 16.0;
    // indent guides
    for d in 0..depth {
        let gx = r.min.x + 13.0 + d as f32 * 16.0;
        ui.painter().line_segment([Pos2::new(gx, r.min.y), Pos2::new(gx, r.max.y)], egui::Stroke::new(1.0, theme::BORDER));
    }
    let mut tx = x;
    if let Some(o) = open {
        let c = Pos2::new(x + 6.0, r.center().y);
        let pts = if o { vec![c + Vec2::new(-4.0, -2.0), c + Vec2::new(0.0, 2.5), c + Vec2::new(4.0, -2.0)] } else { vec![c + Vec2::new(-2.0, -4.0), c + Vec2::new(2.5, 0.0), c + Vec2::new(-2.0, 4.0)] };
        ui.painter().add(egui::Shape::line(pts, egui::Stroke::new(1.4, theme::MUTED)));
        tx += 16.0;
    } else {
        tx += 4.0;
    }
    let fg = if active { theme::TEXT_STRONG } else { color };
    let galley = ui.painter().layout_no_wrap(name.to_string(), FontId::proportional(13.5), fg);
    let clip = Rect::from_min_max(Pos2::new(tx, r.min.y), Pos2::new(r.max.x - if badge.is_empty() { 4.0 } else { 60.0 }, r.max.y));
    ui.painter().with_clip_rect(clip).galley(Pos2::new(tx, r.center().y - galley.size().y / 2.0), galley, fg);
    if !badge.is_empty() {
        let br = Rect::from_min_max(Pos2::new(r.max.x - 58.0, r.center().y - 8.0), Pos2::new(r.max.x - 4.0, r.center().y + 8.0));
        ui.painter().rect_filled(br, CornerRadius::same(4), theme::BG_HOVER);
        ui.painter().text(br.center(), egui::Align2::CENTER_CENTER, badge, FontId::proportional(10.5), theme::MUTED);
    }
    let _ = paint_icon;
    resp
}

/// Order two tree entries. Times are (created, modified); ties fall back to the name.
fn sort_order(sort: crate::config::FileSort, name_a: &str, name_b: &str, ta: (Option<std::time::SystemTime>, Option<std::time::SystemTime>), tb: (Option<std::time::SystemTime>, Option<std::time::SystemTime>)) -> std::cmp::Ordering {
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

    // spec: VAULT-015
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
        assert!(app.expanded.contains(Path::new("Archiv")));
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
        app.left = LeftPanel::Files;
        app.open(Path::new("r1.md"), false);
        app.expanded.clear(); // the start opened a first note (and its folder)
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
            app.tree_sel.clone()
        };
        let p = |s: &str| Some(PathBuf::from(s));
        // Ctrl+H / Ctrl+Shift+B: the file list gets the keyboard with the open note selected
        app.focus_sidebar(&ctx);
        assert_eq!(app.tree_sel, p("r1.md"));
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
        assert!(app.expanded.contains(Path::new("A")));
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
        assert_eq!(app.tree_sel, p("A"));
        press(&mut app, Key::Space, Modifiers::NONE);
        assert!(!app.expanded.contains(Path::new("A")));
        assert_eq!(press(&mut app, Key::J, Modifiers::NONE), p("B/b1.md"));
        app.actions.clear();
        assert_eq!(press(&mut app, Key::Enter, Modifiers::NONE), None, "Enter leaves the file list");
        assert!(matches!(app.actions.first(), Some(Action::Open { rel, .. }) if rel == Path::new("B/b1.md")));
        assert!(app.tabs[app.active].editor.request_focus, "…and the editor gets the keyboard");
        // Esc and Ctrl+L also give the keyboard back; a click elsewhere ends browsing
        app.focus_sidebar(&ctx);
        assert!(app.tree_sel.is_some());
        assert_eq!(press(&mut app, Key::Escape, Modifiers::NONE), None);
        app.focus_sidebar(&ctx);
        app.focus_editor();
        assert_eq!(app.tree_sel, None);
        app.focus_sidebar(&ctx);
        ctx.memory_mut(|m| m.request_focus(egui::Id::unique("somewhere else")));
        assert_eq!(press(&mut app, Key::J, Modifiers::NONE), None);
        // in the search panel, Ctrl+H means the search box
        app.left = LeftPanel::Search;
        app.focus_sidebar(&ctx);
        assert!(app.focus_search);
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod search_tests {
    use crate::app::{Action, Options, ShaleApp};
    use eframe::egui::{self, Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2};
    use std::path::{Path, PathBuf};

    fn key(k: Key, modifiers: Modifiers) -> Event {
        Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers }
    }
    /// Press a key (also sending the letter as typed text, as a real keyboard does).
    fn press(frame: &mut dyn FnMut(&mut ShaleApp, Vec<Event>), app: &mut ShaleApp, k: Key, m: Modifiers) -> Option<usize> {
        let typed = if m.ctrl || !matches!(k, Key::J | Key::K) { vec![] } else { vec![Event::Text(format!("{k:?}").to_lowercase())] };
        frame(app, [vec![key(k, m)], typed].concat());
        app.search_sel
    }
    const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

    /// A vault where "apple" is found: twice in A, by name only in "apple notes", once in B.
    fn browse_app(tag: &str) -> (ShaleApp, PathBuf) {
        let d = std::env::temp_dir().join(format!("shale-search-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("A.md"), "first apple\nno\nsecond Apple here\n").unwrap();
        std::fs::write(d.join("apple notes.md"), "nothing to see\n").unwrap();
        std::fs::write(d.join("B.md"), "  Ünïcode ÄPFEL apple\n").unwrap();
        (ShaleApp::build(Options { screenshot: None, vault: Some(d.clone()), view: None, keys: None }, false), d)
    }

    // spec: NAV-007, NAV-008, NAV-009
    #[test]
    fn search_results_are_browsed_with_j_k_and_ctrl_j_k() {
        let (mut app, d) = browse_app("browse");
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut frame = |app: &mut ShaleApp, events: Vec<Event>| {
            t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))), time: Some(t), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.search_panel(ui));
            });
            out.textures_delta.clear();
        };
        let id = egui::Id::unique("search-input");
        frame(&mut app, vec![]);
        frame(&mut app, vec![Event::Text("apple".into())]);
        assert_eq!(app.search, "apple");
        assert_eq!(app.search_sel, None, "typing in the box is not browsing");
        // ↓ goes into the list: stops are A:0, A:2, "apple notes" (name only), B:0
        frame(&mut app, vec![key(Key::ArrowDown, Modifiers::NONE)]);
        assert_eq!(app.search_sel, Some(0));
        assert!(!ctx.memory(|m| m.has_focus(id)), "the box gives up the keyboard");
        assert_eq!(press(&mut frame, &mut app, Key::J, Modifiers::NONE), Some(1));
        assert_eq!(press(&mut frame, &mut app, Key::J, Modifiers::NONE), Some(2));
        assert_eq!(press(&mut frame, &mut app, Key::J, Modifiers::NONE), Some(3));
        assert_eq!(press(&mut frame, &mut app, Key::J, Modifiers::NONE), Some(3), "stops at the end");
        assert_eq!(press(&mut frame, &mut app, Key::K, Modifiers::NONE), Some(2));
        assert_eq!(app.search, "apple", "j and k did not type into the box");
        // Ctrl+J / Ctrl+K: first stop of the next / previous file
        assert_eq!(press(&mut frame, &mut app, Key::K, CTRL), Some(0), "back to A");
        assert_eq!(press(&mut frame, &mut app, Key::J, CTRL), Some(2), "on to \"apple notes\"");
        assert_eq!(press(&mut frame, &mut app, Key::J, CTRL), Some(3), "on to B");
        assert_eq!(press(&mut frame, &mut app, Key::J, CTRL), Some(3), "no file after B");
        assert_eq!(press(&mut frame, &mut app, Key::ArrowUp, Modifiers::NONE), Some(2), "arrow keys work too");
        assert_eq!(press(&mut frame, &mut app, Key::K, Modifiers::NONE), Some(1));
        // Enter opens the second hit in A at "Apple"
        app.actions.clear();
        press(&mut frame, &mut app, Key::Enter, Modifiers::NONE);
        assert_eq!(app.search_sel, None);
        let Some(Action::GotoMatch { rel, line, col }) = app.actions.pop() else { panic!("Enter opens the hit") };
        assert_eq!((rel.as_path(), line, col), (Path::new("A.md"), 2, 7));
        app.goto_match(&rel, line, col);
        assert_eq!(app.current_path(), Some(PathBuf::from("A.md")));
        let text = &app.notes[Path::new("A.md")].text;
        let cursor = app.tabs[app.active].editor.pending_cursor.expect("cursor placed (and the editor focused)");
        assert!(text.chars().skip(cursor).collect::<String>().starts_with("Apple here"));
        // a name-only stop opens the file; non-ASCII text before the match is counted right
        frame(&mut app, vec![]);
        ctx.memory_mut(|m| m.request_focus(id));
        frame(&mut app, vec![]);
        frame(&mut app, vec![key(Key::ArrowDown, Modifiers::NONE)]);
        press(&mut frame, &mut app, Key::J, CTRL);
        app.actions.clear();
        press(&mut frame, &mut app, Key::Enter, Modifiers::NONE);
        assert!(matches!(app.actions.pop(), Some(Action::Open { rel, .. }) if rel == Path::new("apple notes.md")));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: NAV-007
    #[test]
    fn enter_in_the_box_selects_and_esc_goes_back() {
        let (mut app, d) = browse_app("enter");
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut frame = |app: &mut ShaleApp, events: Vec<Event>| {
            t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))), time: Some(t), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.search_panel(ui));
            });
            out.textures_delta.clear();
        };
        let id = egui::Id::unique("search-input");
        frame(&mut app, vec![]);
        frame(&mut app, vec![Event::Text("äpfel".into())]);
        frame(&mut app, vec![key(Key::Enter, Modifiers::NONE)]);
        assert_eq!(app.search_sel, Some(0), "Enter in the box selects the first hit");
        assert!(app.actions.is_empty(), "…and does not open it yet");
        frame(&mut app, vec![key(Key::Escape, Modifiers::NONE)]);
        frame(&mut app, vec![]);
        assert_eq!(app.search_sel, None);
        assert!(ctx.memory(|m| m.has_focus(id)), "Esc goes back into the box");
        // the match position counts bytes of the original line (Ü, ï are two bytes each)
        let v = app.vault.as_ref().unwrap();
        let hits = v.search("äpfel");
        assert_eq!(hits.len(), 1);
        let (_, col, _) = hits[0].1[0];
        assert_eq!(&"  Ünïcode ÄPFEL apple"[col..], "ÄPFEL apple");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: NAV-004
    #[test]
    fn the_search_shortcut_always_focuses_and_selects_the_old_search() {
        let d = std::env::temp_dir().join(format!("shale-search-focus-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        crate::vault::write_sample_vault(&d).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(d.clone()), view: None, keys: None }, false);
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut frame = |app: &mut ShaleApp, events: Vec<Event>| {
            t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))), time: Some(t), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.search_panel(ui));
            });
            out.textures_delta.clear();
        };
        app.search = "garden".into();
        frame(&mut app, vec![]);
        // focus elsewhere (as after working in a note), then the shortcut
        ctx.memory_mut(|m| m.surrender_focus(egui::Id::unique("search-input")));
        frame(&mut app, vec![]);
        let id = egui::Id::unique("search-input");
        assert!(!ctx.memory(|m| m.has_focus(id)));
        app.focus_search = true;
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        assert!(ctx.memory(|m| m.has_focus(id)), "the box has focus although it isn't empty");
        let range = egui::text_edit::TextEditState::load(&ctx, id).and_then(|s| s.cursor.char_range()).unwrap();
        assert_eq!((range.primary.index.0.min(range.secondary.index.0), range.primary.index.0.max(range.secondary.index.0)), (0, 6), "the old search is selected");
        // typing replaces it
        frame(&mut app, vec![Event::Text("compost".into())]);
        assert_eq!(app.search, "compost");
        // an arrow key keeps it and moves the cursor
        app.focus_search = true;
        frame(&mut app, vec![]);
        frame(&mut app, vec![Event::Key { key: Key::ArrowRight, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        frame(&mut app, vec![Event::Text("s".into())]);
        assert_eq!(app.search, "composts");
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
