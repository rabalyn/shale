//! Sidebars (files, search, tags, linked notes, outline, local graph), the vault-wide
//! to-do list and the dialogs.

use crate::app::{graph_click, muted, Action, LeftPanel, Modal, RightPanel, ShaleApp, View};
use crate::layout::{icon_button, paint_icon, Icon};
use crate::markdown;
use crate::theme;
use crate::vault::{rel_to_string, FileKind};
use eframe::egui::{self, CornerRadius, FontId, Key, Pos2, Rect, Sense, Ui, Vec2};
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
                        let dy = if p.y < clip.top() + 28.0 { 8.0 } else if p.y > clip.bottom() - 28.0 { -8.0 } else { 0.0 };
                        if dy != 0.0 {
                            ui.scroll_with_delta(Vec2::new(0.0, dy));
                            ui.ctx().request_repaint();
                        }
                    }
                }
            }
            let root = PathBuf::new();
            let r = self.tree(ui, &root, 0);
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
            });
            let _ = r;
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

    /// The note (or drawing) before (`step` -1) or after (+1) `rel` in its folder, in the file
    /// list's order. Attachments (PDFs, pictures…) are skipped.
    pub(crate) fn neighbour_note(&self, rel: &Path, step: i32) -> Option<PathBuf> {
        let dir = rel.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let list: Vec<PathBuf> = self.sorted_files_in(&dir).into_iter().filter(|(_, _, k)| *k != FileKind::Other).map(|(p, _, _)| p).collect();
        let i = list.iter().position(|p| p == rel)? as i32 + step;
        (i >= 0).then(|| list.get(i as usize).cloned()).flatten()
    }

    fn tree(&mut self, ui: &mut Ui, dir: &Path, depth: usize) {
        let Some(v) = &self.vault else { return };
        let sort = self.cfg.file_sort;
        let mut folders: Vec<PathBuf> = v.folders.iter().filter(|f| f.parent() == Some(dir)).cloned().collect();
        let fname = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        folders.sort_by(|a, b| {
            let (ta, tb) = (v.folder_times.get(a).copied().unwrap_or_default(), v.folder_times.get(b).copied().unwrap_or_default());
            sort_order(sort, &fname(a), &fname(b), ta, tb)
        });
        let files = self.sorted_files_in(dir);
        let current = self.current_path();
        for f in folders {
            let open = self.expanded.contains(&f);
            let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let resp = tree_row(ui, depth, &name, Some(open), false, theme::TEXT, "");
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
                FileKind::Drawing => "drawing",
                FileKind::Other => rel.extension().and_then(|e| e.to_str()).unwrap_or(""),
                FileKind::Note => "",
            };
            let color = if kind == FileKind::Other { theme::MUTED } else { theme::TEXT };
            let resp = tree_row(ui, depth, &name, None, active, color, badge);
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
        let r = ui.add(egui::TextEdit::singleline(&mut self.search).id(id).hint_text("Search all notes…").desired_width(f32::INFINITY));
        if self.search.is_empty() && !r.has_focus() {
            r.request_focus();
        }
        ui.add_space(6.0);
        let Some(v) = &self.vault else { return };
        let results = v.search(&self.search);
        if !self.search.trim().is_empty() {
            ui.label(muted(format!("{} result{}", results.len(), if results.len() == 1 { "" } else { "s" })).size(12.0));
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (rel, lines) in results {
                let name = v.file(&rel).map(|f| f.display_name()).unwrap_or_default();
                if ui.add(egui::Label::new(egui::RichText::new(&name).color(theme::TEXT_STRONG)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    self.actions.push(Action::Open { rel: rel.clone(), new_tab: false });
                }
                for (line, text) in lines.iter().take(6) {
                    let shown: String = text.chars().take(120).collect();
                    let resp = egui::Frame::new().fill(theme::BG_PRIMARY).corner_radius(CornerRadius::same(4)).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(egui::RichText::new(shown).size(12.5).color(theme::MUTED)).wrap())
                    });
                    if resp.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        self.actions.push(Action::GotoLine { rel: rel.clone(), line: *line });
                    }
                }
                ui.add_space(6.0);
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

    pub fn right_sidebar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for (p, t) in [(RightPanel::Backlinks, "Backlinks"), (RightPanel::Outgoing, "Outgoing"), (RightPanel::Outline, "Outline"), (RightPanel::LocalGraph, "Graph"), (RightPanel::Calendar, "Calendar")] {
                if ui.selectable_label(self.right == p, egui::RichText::new(t).size(12.5)).clicked() {
                    self.right = p;
                }
            }
        });
        ui.separator();
        if self.right == RightPanel::Calendar {
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
        match self.right {
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
                        let resp = ui.add(egui::Label::new(egui::RichText::new(display).color(theme::LINK_MISSING)).sense(Sense::click())).on_hover_text("Click to create this note").on_hover_cursor(egui::CursorIcon::PointingHand);
                        if resp.clicked() {
                            self.actions.push(Action::OpenLink { target, heading: None, from: Some(rel.clone()), new_tab: false });
                        }
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
            let col = (full - 32.0).min(860.0).max(200.0);
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
        let ctx = ui.ctx().clone();
        match &mut modal {
            Modal::QuickSwitch { query, sel } => {
                let resp = egui::Modal::new(egui::Id::unique("quick-switch")).show(&ctx, |ui| {
                    ui.set_width(520.0);
                    let r = ui.add(egui::TextEdit::singleline(query).hint_text("Find or create a note…").desired_width(f32::INFINITY).font(FontId::proportional(16.0)));
                    r.request_focus();
                    let Some(v) = &self.vault else { return };
                    let mut items: Vec<(i32, PathBuf, String)> = v.files.iter().filter_map(|f| markdown::fuzzy_score(query, &rel_to_string(&f.rel)).map(|s| (s, f.rel.clone(), f.display_name()))).collect();
                    items.sort_by(|a, b| b.0.cmp(&a.0).then(a.2.cmp(&b.2)));
                    items.truncate(12);
                    let create = !query.trim().is_empty() && !items.iter().any(|(_, _, n)| n.eq_ignore_ascii_case(query.trim()));
                    let n = items.len() + create as usize;
                    let (down, up, enter, esc) = ui.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, Key::ArrowDown), i.consume_key(egui::Modifiers::NONE, Key::ArrowUp), i.consume_key(egui::Modifiers::NONE, Key::Enter), i.consume_key(egui::Modifiers::NONE, Key::Escape)));
                    if n > 0 {
                        if down {
                            *sel = (*sel + 1) % n;
                        }
                        if up {
                            *sel = (*sel + n - 1) % n;
                        }
                        *sel = (*sel).min(n - 1);
                    }
                    ui.add_space(6.0);
                    let mut pick: Option<usize> = if enter && n > 0 { Some(*sel) } else { None };
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
                    if esc {
                        keep = false;
                    }
                });
                if resp.should_close() {
                    keep = false;
                }
            }
            Modal::Rename { rel, name, error } => {
                let resp = egui::Modal::new(egui::Id::unique("rename")).show(&ctx, |ui| {
                    ui.set_width(420.0);
                    ui.label(egui::RichText::new("Rename").size(16.0));
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
                let resp = egui::Modal::new(egui::Id::unique("new-folder")).show(&ctx, |ui| {
                    ui.set_width(380.0);
                    ui.label(egui::RichText::new("New folder").size(16.0));
                    let r = ui.add(egui::TextEdit::singleline(name).hint_text("Folder name").desired_width(f32::INFINITY));
                    r.request_focus();
                    let enter = ui.input(|i| i.key_pressed(Key::Enter));
                    ui.horizontal(|ui| {
                        if (ui.button("Create").clicked() || enter) && !name.trim().is_empty() {
                            let rel = parent.join(markdown::sanitize_note_name(name));
                            if let Some(v) = self.vault.as_mut() {
                                match v.create_folder(&rel) {
                                    Ok(()) => {
                                        self.expanded.insert(rel.clone());
                                        if let Some(p) = rel.parent() {
                                            self.expanded.insert(p.to_path_buf());
                                        }
                                    }
                                    Err(e) => self.toasts.push((format!("Could not create folder: {e}"), std::time::Instant::now())),
                                }
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
                let resp = egui::Modal::new(egui::Id::unique("trash")).show(&ctx, |ui| {
                    ui.set_width(400.0);
                    ui.label(egui::RichText::new("Move to trash?").size(16.0));
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
            Modal::Export(_) | Modal::Calendars(_) | Modal::Event(_) | Modal::Import(_) => {}
            Modal::Settings { vault_path } => {
                let resp = egui::Modal::new(egui::Id::unique("settings")).show(&ctx, |ui| {
                    ui.set_width(520.0);
                    ui.label(egui::RichText::new("Settings").size(18.0));
                    ui.add_space(6.0);
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
                        changed |= ui.add(egui::TextEdit::singleline(&mut self.cfg.people_dir).hint_text(crate::people::DEFAULT_DIR).desired_width(220.0)).on_hover_text("Contact notes live here. Type [[p and a name to link a person; imported e-mails and events find their people here.").changed();
                    });
                    ui.label(muted("Type [[p followed by a name to pick a person from this folder (or make a new contact note).").size(12.0));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Appearance").color(theme::MUTED));
                    changed |= ui.checkbox(&mut self.cfg.readable_width, "Readable line length").changed();
                    changed |= ui.checkbox(&mut self.cfg.dark_canvas, "Dark drawing canvas").changed();
                    changed |= ui.checkbox(&mut self.cfg.inline_images, "Show embedded images inside the editor (the link text appears while the cursor is on it)").changed();
                    ui.horizontal(|ui| {
                        ui.label("Text size");
                        changed |= ui.add(egui::Slider::new(&mut self.cfg.font_size, 12.0..=24.0)).changed();
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
                    ui.add_space(10.0);
                    if ui.button("Done").clicked() {
                        keep = false;
                    }
                });
                if resp.should_close() {
                    keep = false;
                }
            }
        }
        if open_calendars {
            self.open_calendar_settings();
            keep = false;
        }
        if let Modal::Calendars(d) = &mut modal {
            let mut out = None;
            let resp = egui::Modal::new(egui::Id::unique("calendars")).show(&ctx, |ui| {
                let h = ctx.content_rect().height() - 80.0;
                egui::ScrollArea::vertical().max_height(h).min_scrolled_height(h).show(ui, |ui| {
                    out = crate::cal_dialog::show(ui, d, &self.calendar);
                });
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
        if let Modal::Import(d) = &mut modal {
            let folders = self.import_folders();
            let tags = self.tag_choices();
            let resp = egui::Modal::new(egui::Id::unique("import")).show(&ctx, |ui| crate::import_dialog::show(ui, d, &folders, &tags));
            let close_asked = resp.should_close();
            if let Some(a) = resp.inner {
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
            let resp = egui::Modal::new(egui::Id::unique("event")).show(&ctx, |ui| crate::cal_dialog::show_event(ui, d, store));
            let close_asked = resp.should_close();
            match resp.inner {
                Some(crate::cal_dialog::EventResult::Saved(msg)) => {
                    self.last_event_calendar = Some(d.calendar.clone());
                    self.toast(msg);
                    keep = false;
                }
                Some(crate::cal_dialog::EventResult::Cancel) => keep = false,
                None => {}
            }
            // Esc / clicking outside closes it, but not while a save is still running
            if close_asked && !d.is_saving() {
                keep = false;
            }
        }
        if let Modal::Export(d) = &mut modal {
            let resp = egui::Modal::new(egui::Id::unique("export-pdf")).show(&ctx, |ui| crate::export_dialog::show(ui, d));
            match resp.inner {
                crate::export_dialog::DialogResult::Export => match self.run_export(d) {
                    Ok(out) => {
                        self.toast(format!("Exported to {}", out.display()));
                        if d.open_after {
                            let _ = std::process::Command::new("xdg-open").arg(&out).spawn();
                        }
                        keep = false;
                    }
                    Err(e) => d.error = Some(e),
                },
                crate::export_dialog::DialogResult::Cancel => keep = false,
                crate::export_dialog::DialogResult::Open => {}
            }
            if resp.should_close() {
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
fn sort_order(
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
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
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

    #[test]
    fn due_is_stripped() {
        assert_eq!(super::strip_due("Write 📅 2026-12-01"), "Write");
        assert_eq!(super::strip_due("Pay due:2026-01-02 rent"), "Pay rent");
        assert_eq!(super::strip_due("plain"), "plain");
    }

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
mod neighbour_tests {
    use crate::app::{Options, ShaleApp};
    use crate::config::FileSort;
    use std::path::{Path, PathBuf};

    #[test]
    fn ctrl_h_and_l_follow_the_file_list_order() {
        let d = std::env::temp_dir().join(format!("shale-neighbour-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        crate::vault::write_sample_vault(&d).unwrap();
        std::fs::write(d.join("photo.png"), b"x").unwrap();
        // modified times: Welcome oldest … Daily log newest
        let base = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000);
        for (i, f) in ["Welcome.md", "Sketch.excalidraw", "photo.png", "Ideas.md", "Daily log.md"].iter().enumerate() {
            std::fs::File::options().write(true).open(d.join(f)).unwrap().set_modified(base + std::time::Duration::from_secs(i as u64 * 60)).unwrap();
        }
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(d.clone()), view: None, keys: None }, false);
        app.vault.as_mut().unwrap().rescan();
        let p = |s: &str| Some(PathBuf::from(s));
        app.cfg.file_sort = FileSort::NameAsc;
        assert_eq!(app.neighbour_note(Path::new("Ideas.md"), 1), p("Sketch.excalidraw"));
        assert_eq!(app.neighbour_note(Path::new("Sketch.excalidraw"), 1), p("Welcome.md"), "the picture in between is skipped");
        assert_eq!(app.neighbour_note(Path::new("Ideas.md"), -1), p("Daily log.md"));
        assert_eq!(app.neighbour_note(Path::new("Daily log.md"), -1), None, "first in the folder");
        assert_eq!(app.neighbour_note(Path::new("Welcome.md"), 1), None, "last in the folder");
        assert_eq!(app.neighbour_note(Path::new("Projects/Garden plan.md"), 1), None, "only notes of the same folder");
        app.cfg.file_sort = FileSort::ModifiedDesc;
        assert_eq!(app.neighbour_note(Path::new("Daily log.md"), 1), p("Ideas.md"), "newest first");
        assert_eq!(app.neighbour_note(Path::new("Ideas.md"), 1), p("Sketch.excalidraw"));
        assert_eq!(app.neighbour_note(Path::new("Sketch.excalidraw"), 1), p("Welcome.md"));
        // the shortcut action opens it in the current tab
        app.open(Path::new("Daily log.md"), false);
        app.open_neighbour(1);
        assert_eq!(app.current_path(), p("Ideas.md"));
        app.open_neighbour(-1);
        assert_eq!(app.current_path(), p("Daily log.md"));
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod search_tests {
    use crate::app::{Options, ShaleApp};
    use eframe::egui::{self, Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2};

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
