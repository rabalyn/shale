//! Drag and drop into the app (e-mails, calendar events, files, text) and the Import dialog.

use super::{Action, Modal, ShaleApp};
use crate::import::{self, Item};
use crate::import_dialog::{ImportAction, ImportDialog};
use crate::theme;
use crate::vault::rel_to_string;
use eframe::egui::{self, Color32, CornerRadius, FontId, Pos2, Rect, Stroke, Ui, Vec2};
use std::path::{Path, PathBuf};

/// A place on screen something can be dropped on: new notes go to `folder`, and (if set)
/// links to them are added to `note`.
#[derive(Clone, Debug)]
pub struct DropZone {
    pub rect: Rect,
    pub folder: PathBuf,
    pub note: Option<PathBuf>,
}

impl ShaleApp {
    /// Remember where things can be dropped this frame (the smallest zone under the pointer wins).
    pub fn add_drop_zone(&mut self, rect: Rect, folder: PathBuf, note: Option<PathBuf>) {
        if rect.is_positive() {
            self.drop_zones.push(DropZone { rect, folder, note });
        }
    }

    /// Calendar areas: an event dropped back on them is not imported.
    pub fn add_calendar_area(&mut self, rect: Rect) {
        self.calendar_areas.push(rect);
    }

    fn zone_at(&self, p: Pos2) -> Option<DropZone> {
        self.drop_zones.iter().filter(|z| z.rect.contains(p)).min_by(|a, b| a.rect.area().total_cmp(&b.rect.area())).cloned()
    }

    /// The vault's folders for the folder picker (without the media folder and hidden ones).
    pub fn import_folders(&self) -> Vec<String> {
        let Some(v) = &self.vault else { return vec![] };
        let mut f: Vec<String> = v.folders.iter().map(|p| rel_to_string(p)).filter(|s| !s.is_empty() && !s.starts_with(crate::media::MEDIA_DIR) && !s.split('/').any(|p| p.starts_with('.'))).collect();
        f.sort_by_key(|s| s.to_lowercase());
        f
    }

    /// Tags to offer: the recently used ones, then the vault's most used ones.
    pub fn tag_choices(&self) -> Vec<String> {
        let mut out: Vec<String> = self.cfg.recent_tags.clone();
        if let Some(v) = &self.vault {
            let mut all = v.all_tags();
            all.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            for (t, _) in all {
                if !out.iter().any(|o| o.eq_ignore_ascii_case(&t)) {
                    out.push(t);
                }
            }
        }
        out.truncate(24);
        out
    }

    /// Show the Import dialog with these items (or add them to the open one).
    /// `zone`: where they were dropped (folder, and a note to link them from).
    pub fn open_import(&mut self, mut items: Vec<Item>, notes: Vec<String>, zone: Option<(PathBuf, Option<PathBuf>)>) {
        let Some(v) = &self.vault else {
            self.toast("Open a vault first to import into it.");
            return;
        };
        import::mark_duplicates(v, &mut items);
        if let Some(Modal::Import(d)) = &mut self.modal {
            d.items.extend(items);
            d.notes.extend(notes);
            if let Some((folder, note)) = zone {
                d.folder = rel_to_string(&folder);
                if note.is_some() {
                    d.link_into = note;
                    d.link_on = true;
                }
            }
            return;
        }
        let (folder, link_into, link_on) = match zone {
            Some((f, note)) => (rel_to_string(&f), note.clone(), note.is_some()),
            None => {
                let open_note = self.current_path().filter(|p| p.extension().map(|e| e == "md").unwrap_or(false));
                let folder = if self.cfg.last_import_folder.is_empty() { rel_to_string(&self.current_folder()) } else { self.cfg.last_import_folder.clone() };
                (folder, open_note, false)
            }
        };
        self.modal = Some(Modal::Import(Box::new(ImportDialog::new(items, notes, folder, String::new(), link_into, link_on))));
    }

    /// Apply what the Import dialog asked for. True when it should close.
    pub fn import_action(&mut self, d: &mut ImportDialog, a: ImportAction) -> bool {
        match a {
            ImportAction::Cancel => true,
            ImportAction::AddPath => {
                let p = PathBuf::from(crate::layout::shellexpand(&d.add_path));
                let (mut items, notes) = import::items_from_paths(&[p]);
                if let Some(v) = &self.vault {
                    import::mark_duplicates(v, &mut items);
                }
                if !items.is_empty() {
                    d.add_path.clear();
                }
                d.items.extend(items);
                d.notes = notes;
                false
            }
            ImportAction::Import => {
                let folder = match import::clean_folder(&d.folder) {
                    Ok(f) => f,
                    Err(e) => {
                        d.error = Some(e);
                        return false;
                    }
                };
                let tags = import::parse_tags(&d.tags);
                let opts = import::Options { folder: folder.clone(), tags: tags.clone(), keep_original: d.keep_original };
                let Some(v) = &mut self.vault else { return true };
                let (made, errors) = import::import(v, &d.items, &opts);
                if made.is_empty() {
                    d.error = Some(if errors.is_empty() { "Nothing was imported.".into() } else { errors.join("\n") });
                    return false;
                }
                if d.link_on {
                    if let Some(note) = d.link_into.clone() {
                        self.add_links(&note, &made);
                    }
                }
                // remember the folder and tags for next time
                self.cfg.last_import_folder = rel_to_string(&folder);
                for t in tags.iter().rev() {
                    self.cfg.recent_tags.retain(|o| !o.eq_ignore_ascii_case(t));
                    self.cfg.recent_tags.insert(0, t.clone());
                }
                self.cfg.recent_tags.truncate(30);
                self.save_config();
                let place = if folder.as_os_str().is_empty() { "the vault".to_string() } else { format!("“{}”", rel_to_string(&folder)) };
                self.toast(if made.len() == 1 { format!("Imported 1 note into {place}") } else { format!("Imported {} notes into {place}", made.len()) });
                for e in errors {
                    self.toast(e);
                }
                self.expanded.insert(folder.clone());
                let mut p = folder.parent();
                while let Some(x) = p {
                    if !x.as_os_str().is_empty() {
                        self.expanded.insert(x.to_path_buf());
                    }
                    p = x.parent();
                }
                self.actions.push(Action::Rescan);
                self.actions.push(Action::Open { rel: made[0].clone(), new_tab: false });
                true
            }
        }
    }

    /// Add "- [[note]]" lines for the new notes at the end of `note` (also when it is open).
    fn add_links(&mut self, note: &Path, made: &[PathBuf]) {
        let lines: String = made.iter().map(|m| format!("- [[{}]]\n", crate::markdown::strip_md_ext(&m.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))).collect();
        let join = |text: &str| -> String {
            let mut t = text.to_string();
            if !t.is_empty() && !t.ends_with('\n') {
                t.push('\n');
            }
            t.push_str(&lines);
            t
        };
        if let Some(doc) = self.notes.get_mut(note) {
            doc.text = join(&doc.text);
            doc.dirty = true;
            doc.last_edit = std::time::Instant::now() - std::time::Duration::from_secs(10);
            return;
        }
        let Some(v) = &mut self.vault else { return };
        let old = std::fs::read_to_string(v.abs(note)).unwrap_or_default();
        if let Err(e) = v.write_note(note, &join(&old)) {
            self.toast(format!("Could not add the links: {e}"));
        }
    }

    /// Take drops (files from the window system, data from Wayland, events dragged out of the
    /// calendar) and paint the drop highlight. Call after the layout of the frame.
    pub fn handle_drops(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let zoom = ctx.zoom_factor();
        // files dropped through the window system (X11)
        let (dropped, hovering_files) = ctx.input(|i| (i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect::<Vec<_>>(), !i.raw.hovered_files.is_empty()));
        if !dropped.is_empty() {
            let (items, notes) = import::items_from_paths(&dropped);
            self.open_import(items, notes, None);
        }
        // drops from other programs on Wayland (and scripted test drops)
        #[cfg(target_os = "linux")]
        {
            let mut events = crate::wl_drop::take_events();
            events.append(&mut self.fake_drops);
            for e in events {
                match e {
                    crate::wl_drop::DropEvent::Hover { x, y } => self.ext_hover = Some(Pos2::new(x / zoom, y / zoom)),
                    crate::wl_drop::DropEvent::Leave => self.ext_hover = None,
                    crate::wl_drop::DropEvent::Busy => {
                        self.ext_hover = None;
                        self.drop_busy = true;
                    }
                    crate::wl_drop::DropEvent::Refused { offered } => {
                        self.ext_hover = None;
                        self.last_offered = offered;
                    }
                    crate::wl_drop::DropEvent::Drop { x, y, parts, offered, report } => {
                        self.ext_hover = None;
                        self.drop_busy = false;
                        self.last_offered = offered;
                        let zone = self.zone_at(Pos2::new(x / zoom, y / zoom)).map(|z| (z.folder, z.note));
                        // looking a message up in Thunderbird's folders can take a moment
                        let (tx, rx) = std::sync::mpsc::channel();
                        let c = ctx.clone();
                        std::thread::spawn(move || {
                            let (items, mut notes) = import::items_from_parts(&parts);
                            if items.is_empty() && !report.is_empty() {
                                // the exact story of each kind instead of just "(empty)"
                                notes.retain(|n| !n.starts_with("  "));
                                notes.extend(report);
                            }
                            let _ = tx.send((items, notes));
                            c.request_repaint();
                        });
                        self.pending_drops.push((rx, zone));
                    }
                }
            }
            if !self.last_offered.is_empty() && self.ext_hover.is_none() && !matches!(self.modal, Some(Modal::Import(_))) {
                // a drag the app couldn't take: say what was offered, once
                let offered = std::mem::take(&mut self.last_offered);
                if !offered.iter().any(|o| crate::wl_drop::pick_mime(std::slice::from_ref(o)).is_some()) {
                    self.toast(format!("That can't be dropped here (it offered {}).", offered.join(", ")));
                }
            }
        }
        // finished drops
        let mut done = Vec::new();
        self.pending_drops.retain(|(rx, zone)| match rx.try_recv() {
            Ok(r) => {
                done.push((r, zone.clone()));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(_) => false,
        });
        for ((items, mut notes), zone) in done {
            if items.is_empty() && !self.last_offered.is_empty() {
                notes.push(format!("It offered: {}", self.last_offered.join(", ")));
                if self.last_offered.iter().any(|o| o == "DELETE" || o == "STRING") {
                    notes.push("That program runs through X11 (XWayland). If it still sends nothing, start it as a Wayland program (for Thunderbird: MOZ_ENABLE_WAYLAND=1), or drag the message to a folder first and drop the .eml file.".into());
                }
            }
            self.last_offered.clear();
            self.open_import(items, notes, zone);
        }
        if !self.pending_drops.is_empty() || self.drop_busy {
            let p = ctx.content_rect().center_bottom() - Vec2::new(90.0, 60.0);
            self.paint_hint_text(&ctx, p, "Reading the dropped message…");
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // an event dragged out of the calendar
        let cal = egui::DragAndDrop::payload::<crate::cal_ui::CalDrag>(&ctx);
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        if let (Some(drag), Some(p)) = (&cal, pointer) {
            let over_calendar = self.calendar_areas.iter().any(|r| r.contains(p));
            let zone = if over_calendar { None } else { self.zone_at(p) };
            if ctx.input(|i| i.pointer.any_released()) {
                egui::DragAndDrop::clear_payload(&ctx);
                if !over_calendar {
                    let file = format!("{}.ics", if drag.event.summary.trim().is_empty() { "event".to_string() } else { drag.event.summary.replace(['/', '\\'], "-") });
                    let item = import::event_item(drag.event.clone(), drag.calendar.clone(), Some((file, crate::cal_ics::to_ics("", std::slice::from_ref(&drag.event)).into_bytes())));
                    self.open_import(vec![item], vec![], zone.map(|z| (z.folder, z.note)));
                }
            } else if over_calendar {
                self.paint_hint_text(&ctx, p, &format!("Event: {}\ndrag it onto a note, a folder or the editor to import it", drag.event.summary));
            } else {
                let label = format!("Event: {}", if drag.event.summary.is_empty() { "(no title)" } else { &drag.event.summary });
                self.paint_drop_hint(&ctx, p, zone.as_ref(), &label);
            }
        }
        let ext = self.ext_hover.or(if hovering_files { pointer } else { None });
        if let Some(p) = ext {
            let zone = self.zone_at(p);
            self.paint_drop_hint(&ctx, p, zone.as_ref(), "Drop to import");
            ctx.request_repaint();
        }
    }

    fn paint_drop_hint(&self, ctx: &egui::Context, p: Pos2, zone: Option<&DropZone>, what: &str) {
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("drop-hint")));
        let target = match zone {
            Some(z) => {
                painter.rect_filled(z.rect, CornerRadius::same(4), theme::ACCENT.gamma_multiply(0.12));
                painter.rect_stroke(z.rect, CornerRadius::same(4), Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
                let folder = if z.folder.as_os_str().is_empty() { "the vault".to_string() } else { format!("“{}”", rel_to_string(&z.folder)) };
                match &z.note {
                    Some(n) => format!("into {folder}, linked from “{}”", crate::markdown::strip_md_ext(&n.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())),
                    None => format!("into {folder}"),
                }
            }
            None => "into the folder used last (or drop it on a note or folder)".to_string(),
        };
        self.paint_hint_text(ctx, p, &format!("{what}\n{target}"));
    }

    fn paint_hint_text(&self, ctx: &egui::Context, p: Pos2, text: &str) {
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("drop-hint")));
        let text = text.to_string();
        let galley = painter.layout_no_wrap(text, FontId::proportional(13.0), theme::TEXT_STRONG);
        let r = Rect::from_min_size(p + Vec2::new(16.0, 12.0), galley.size() + Vec2::new(16.0, 10.0));
        painter.rect_filled(r, CornerRadius::same(6), theme::BG_SECONDARY);
        painter.rect_stroke(r, CornerRadius::same(6), Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Inside);
        painter.galley(r.min + Vec2::new(8.0, 5.0), galley, Color32::WHITE);
    }
}
