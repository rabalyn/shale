//! Drag and drop into the app (e-mails, calendar events, files, text) and the Import dialog.

use super::{Action, Modal, ShaleApp};
use crate::import::dialog::{ImportAction, ImportDialog};
use crate::import::{self, Item};
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

/// A running import of dropped things: where its items arrive, and the folder zone it was dropped on
/// (the folder, and the note inside it if any).
pub(crate) type PendingDrop = (std::sync::mpsc::Receiver<Result<(Vec<crate::import::Item>, Vec<String>), String>>, Option<(PathBuf, Option<PathBuf>)>);

/// The drag-and-drop state of the window: where things can be dropped this frame, what is dragged
/// over the window, and the drops being read in the background.
#[derive(Default)]
pub(crate) struct DragState {
    /// where things can be dropped this frame
    pub(crate) drop_zones: Vec<DropZone>,
    /// file list drop targets this frame: (row, folder to move into, a closed folder row)
    pub(crate) tree_targets: Vec<(Rect, PathBuf, bool)>,
    /// a closed folder hovered while dragging, since when (it opens after a moment)
    pub(crate) tree_hover: Option<(PathBuf, std::time::Instant)>,
    pub(crate) calendar_areas: Vec<Rect>,
    /// something from another program is dragged over the window (Wayland)
    pub(crate) ext_hover: Option<Pos2>,
    /// the kinds the last drag from another program offered
    pub(crate) last_offered: Vec<String>,
    /// something Shale can't take is dragged over the window: where, and what it offers
    #[cfg(target_os = "linux")]
    pub(crate) ext_refused: Option<(Pos2, Vec<String>)>,
    /// test drops from `--keys` (handled like drops from other programs)
    #[cfg(target_os = "linux")]
    pub(crate) fake_drops: Vec<crate::import::wl_drop::DropEvent>,
    /// imports of drops being read in the background
    pub(crate) pending_drops: Vec<PendingDrop>,
    /// a drop from another program is being read
    pub(crate) drop_busy: bool,
}

impl DragState {
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

    pub(crate) fn zone_at(&self, p: Pos2) -> Option<DropZone> {
        self.drop_zones.iter().filter(|z| z.rect.contains(p)).min_by(|a, b| a.rect.area().total_cmp(&b.rect.area())).cloned()
    }
}

impl ShaleApp {
    /// The vault's folders for the folder picker (without the media folder and hidden ones).
    pub fn import_folders(&self) -> Vec<String> {
        let Some(v) = &self.vault else { return vec![] };
        let mut f: Vec<String> = v.folders.iter().map(|p| rel_to_string(p)).filter(|s| !s.is_empty() && !s.starts_with(crate::media::MEDIA_DIR) && !s.split('/').any(|p| p.starts_with('.'))).collect();
        f.sort_by_key(|s| s.to_lowercase());
        f
    }

    /// Tags to offer: the recently used ones, then the vault's most used ones.
    pub fn tag_choices(&self) -> Vec<String> {
        let mut out: Vec<String> = self.vdata.state.recent_tags.clone();
        if let Some(v) = &self.vault {
            let mut all = v.all_tags();
            all.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            for (t, _) in all {
                if t != "person" && !out.iter().any(|o| o.eq_ignore_ascii_case(&t)) {
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
        import::find_people(v, &self.vdata.settings.people_dir, &mut items);
        if let Some(Modal::Import(mut d)) = self.modal.take() {
            d.items.extend(items);
            d.notes.extend(notes);
            if let Some((folder, note)) = zone {
                d.folder = rel_to_string(&folder);
                if note.is_some() {
                    d.link_into = note;
                    d.link_on = true;
                }
            }
            self.suggest_tags(&mut d);
            self.modal = Some(Modal::Import(d));
            return;
        }
        let (folder, link_into, link_on) = match zone {
            Some((f, note)) => (rel_to_string(&f), note.clone(), note.is_some()),
            None => {
                let open_note = self.current_path().filter(|p| p.extension().map(|e| e == "md").unwrap_or(false));
                let mut folder = if self.vdata.state.last_import_folder.is_empty() { rel_to_string(&self.current_folder()) } else { self.vdata.state.last_import_folder.clone() };
                // not into the contacts because a contact note happens to be open
                let pd = self.vdata.settings.people_dir.trim().trim_matches('/').to_lowercase();
                if !pd.is_empty() && (folder.to_lowercase() == pd || folder.to_lowercase().starts_with(&format!("{pd}/"))) {
                    folder.clear();
                }
                (folder, open_note, false)
            }
        };
        let mut d = Box::new(ImportDialog::new(items, notes, folder, String::new(), link_into, link_on));
        d.people_dir = self.vdata.settings.people_dir.clone();
        self.suggest_tags(&mut d);
        self.modal = Some(Modal::Import(d));
    }

    /// Tags for the dialog's items: the vault's tags written in them are put in right away, the
    /// learned ones offered as suggestions.
    pub fn suggest_tags(&self, d: &mut ImportDialog) {
        let Some(v) = &self.vault else { return };
        let text: String = d.items.iter().filter(|i| i.include).map(|i| i.tag_text()).collect::<Vec<_>>().join("\n");
        let known: Vec<String> = v.all_tags().into_iter().map(|t| t.0).filter(|t| !t.eq_ignore_ascii_case("person")).collect();
        let mut tags = import::parse_tags(&d.tags);
        for t in crate::import::tagwords::direct(&known, &text) {
            if !d.auto_tags.contains(&t) {
                d.auto_tags.push(t.clone());
                if !tags.iter().any(|x| x.eq_ignore_ascii_case(&t)) {
                    tags.push(t);
                }
            }
        }
        d.tags = tags.join(", ");
        d.suggested = self.tag_words.as_ref().map(|tw| crate::import::tagwords::suggest(tw, &text, &d.auto_tags)).unwrap_or_default();
    }

    /// Learn the tag words again now and then (in the background), and keep them in the vault.
    pub fn learn_tags_tick(&mut self) {
        if let Some(rx) = &self.tag_job {
            match rx.try_recv() {
                Ok(Err(e)) => {
                    self.tag_job = None;
                    self.toast(format!("Learning tag words failed: {e}"));
                }
                Ok(Ok(tw)) => {
                    let saved = self.vault.as_ref().map(|v| crate::import::tagwords::save(&v.root, &tw));
                    if let Some(Err(e)) = saved {
                        self.toast(format!("Could not save the tag words: {e}"));
                    }
                    self.tag_words = Some(tw);
                    self.tag_job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.tag_job = None,
                Err(_) => {}
            }
        }
        let Some(v) = &self.vault else { return };
        if self.tag_root.as_deref() != Some(v.root.as_path()) {
            // another vault: start with what was learned there before
            self.tag_root = Some(v.root.clone());
            self.tag_words = crate::import::tagwords::load(&v.root);
            self.tag_gen = u64::MAX;
            self.tag_at = None;
        }
        let due = self.tag_job.is_none() && v.generation != self.tag_gen && self.tag_at.map(|t| t.elapsed() > std::time::Duration::from_secs(120)).unwrap_or(true);
        if due {
            self.tag_gen = v.generation;
            self.tag_at = Some(std::time::Instant::now());
            let notes: Vec<(String, Vec<String>)> = v.notes.values().map(|n| (n.text.clone(), n.tags.clone())).collect();
            self.tag_job = Some(crate::jobs::spawn(move || crate::import::tagwords::learn_from_vault(&notes), || {}));
        }
    }

    /// A contact note for `[[p Name` (made in the people folder).
    pub fn create_person(&mut self, name: &str) {
        let dir = self.vdata.settings.people_dir.clone();
        let Some(v) = &mut self.vault else { return };
        let p = crate::import::people::Person { name: name.trim().to_string(), email: String::new(), role: crate::import::people::Role::From, existing: None, on: true };
        match crate::import::people::ensure_note(v, &dir, &p) {
            Ok(rel) => self.toast(format!("Contact note “{}” made", rel_to_string(&rel))),
            Err(e) => self.toast(e),
        }
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
                if let Some(v) = &self.vault {
                    import::find_people(v, &self.vdata.settings.people_dir, &mut items);
                }
                if !items.is_empty() {
                    d.add_path.clear();
                }
                d.items.extend(items);
                self.suggest_tags(d);
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
                let social_only = d.items.iter().filter(|i| i.include).all(|i| matches!(i.source, import::Source::Social { .. }));
                let opts = import::Options { folder: folder.clone(), tags: tags.clone(), keep_original: d.keep_original, people_dir: self.vdata.settings.people_dir.clone() };
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
                // remember whether replies of others were wanted (only asked when there were some)
                if let Some(on) = d
                    .items
                    .iter()
                    .filter(|i| i.include)
                    .filter_map(|i| match &i.source {
                        import::Source::Social { saved, replies, .. } if !saved.replies.is_empty() => Some(*replies),
                        _ => None,
                    })
                    .next_back()
                {
                    self.cfg.social_replies = on;
                }
                if social_only {
                    self.vdata.state.last_social_folder = rel_to_string(&folder);
                } else {
                    self.vdata.state.last_import_folder = rel_to_string(&folder);
                }
                for t in tags.iter().rev() {
                    self.vdata.state.recent_tags.retain(|o| !o.eq_ignore_ascii_case(t));
                    self.vdata.state.recent_tags.insert(0, t.clone());
                }
                self.vdata.state.recent_tags.truncate(30);
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
        let Some(_tree) = self.hold_sync_tree() else { return };
        let Some(v) = &mut self.vault else { return };
        // the file as it is now; one that can't be read is left alone
        if let Err(e) = v.edit_note(note, |old| Some(join(old))) {
            self.toast(format!("Could not add the links: {e}"));
        }
    }

    /// Take drops (files from the window system, data from Wayland, events dragged out of the
    /// calendar) and paint the drop highlight. Call after the layout of the frame.
    pub fn handle_drops(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        // files dropped through the window system (X11, macOS, Windows)
        let (dropped, hovering_files) = ctx.input(|i| (i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect::<Vec<_>>(), !i.raw.hovered_files.is_empty()));
        if !dropped.is_empty() {
            let (items, notes) = import::items_from_paths(&dropped);
            self.open_import(items, notes, None);
        }
        // drops from other programs on Wayland (and scripted test drops)
        #[cfg(target_os = "linux")]
        {
            let zoom = ctx.zoom_factor();
            let mut events = crate::import::wl_drop::take_events();
            events.append(&mut self.drag.fake_drops);
            for e in events {
                match e {
                    crate::import::wl_drop::DropEvent::Hover { x, y } => self.drag.ext_hover = Some(Pos2::new(x / zoom, y / zoom)),
                    crate::import::wl_drop::DropEvent::Leave => {
                        self.drag.ext_hover = None;
                        self.drag.ext_refused = None;
                    }
                    crate::import::wl_drop::DropEvent::Busy => {
                        self.drag.ext_hover = None;
                        self.drag.drop_busy = true;
                    }
                    crate::import::wl_drop::DropEvent::Refused { x, y, offered } => {
                        // only a hint at the pointer while it is over the window: passing over
                        // the window with a browser tab is not worth a message
                        self.drag.ext_hover = None;
                        self.drag.ext_refused = Some((Pos2::new(x / zoom, y / zoom), offered));
                    }
                    crate::import::wl_drop::DropEvent::Drop { x, y, parts, offered, report } => {
                        self.drag.ext_hover = None;
                        self.drag.drop_busy = false;
                        self.drag.last_offered = offered;
                        let zone = self.drag.zone_at(Pos2::new(x / zoom, y / zoom)).map(|z| (z.folder, z.note));
                        // looking a message up in Thunderbird's folders can take a moment
                        let c = ctx.clone();
                        let rx = crate::jobs::spawn(
                            move || {
                                let (items, mut notes) = import::items_from_parts(&parts);
                                if items.is_empty() && !report.is_empty() {
                                    // the exact story of each kind instead of just "(empty)"
                                    notes.retain(|n| !n.starts_with("  "));
                                    notes.extend(report);
                                }
                                (items, notes)
                            },
                            move || c.request_repaint(),
                        );
                        self.drag.pending_drops.push((rx, zone));
                    }
                }
            }
            if let Some((p, offered)) = &self.drag.ext_refused {
                let shown: Vec<&str> = offered.iter().take(4).map(|s| s.as_str()).collect();
                self.paint_hint_text(&ctx, *p, &format!("Shale can't take this\n(it offers {}{})", shown.join(", "), if offered.len() > 4 { ", …" } else { "" }));
            }
        }
        // finished drops
        let mut done = Vec::new();
        self.drag.pending_drops.retain(|(rx, zone)| match rx.try_recv() {
            Ok(r) => {
                // a crash while reading the drop ends as a message in the import dialog
                done.push((r.unwrap_or_else(|e| (vec![], vec![format!("Reading the drop failed: {e}")])), zone.clone()));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(_) => false,
        });
        for ((items, mut notes), zone) in done {
            if items.is_empty() && !self.drag.last_offered.is_empty() {
                notes.push(format!("It offered: {}", self.drag.last_offered.join(", ")));
                if self.drag.last_offered.iter().any(|o| o == "DELETE" || o == "STRING") {
                    notes.push(
                        "That program runs through X11 (XWayland). If it still sends nothing, start it as a Wayland program (for Thunderbird: MOZ_ENABLE_WAYLAND=1), or drag the message to a folder first and drop the .eml file.".into(),
                    );
                }
            }
            self.drag.last_offered.clear();
            self.open_import(items, notes, zone);
        }
        if !self.drag.pending_drops.is_empty() || self.drag.drop_busy {
            let p = ctx.content_rect().center_bottom() - Vec2::new(90.0, 60.0);
            self.paint_hint_text(&ctx, p, "Reading the dropped message…");
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // a file or folder dragged in the file list
        if let (Some(drag), Some(p)) = (egui::DragAndDrop::payload::<crate::panels::TreeDrag>(&ctx), pointer_now(&ctx)) {
            let target = self.drag.tree_targets.iter().filter(|(r, _, _)| r.contains(p)).min_by(|a, b| a.0.area().total_cmp(&b.0.area())).cloned();
            let from = drag.0.clone();
            let name = from.file_name().map(|n| crate::markdown::strip_md_ext(&n.to_string_lossy()).to_string()).unwrap_or_default();
            let here = from.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let verdict = target.as_ref().map(|(_, into, _)| {
                if *into == here {
                    Err("already here")
                } else if into.starts_with(&from) {
                    Err("a folder can't go into itself")
                } else {
                    Ok(())
                }
            });
            // a closed folder opens when the pointer rests on it
            match &target {
                Some((_, f, true)) => match &self.drag.tree_hover {
                    Some((h, since)) if h == f => {
                        if since.elapsed() > std::time::Duration::from_millis(700) {
                            self.expanded.insert(f.clone());
                            self.drag.tree_hover = None;
                        }
                    }
                    _ => self.drag.tree_hover = Some((f.clone(), std::time::Instant::now())),
                },
                _ => self.drag.tree_hover = None,
            }
            if ctx.input(|i| i.pointer.any_released()) {
                egui::DragAndDrop::clear_payload(&ctx);
                self.drag.tree_hover = None;
                if let (Some((_, into, _)), Some(Ok(()))) = (target, verdict) {
                    self.actions.push(Action::Move { from, into });
                }
            } else {
                let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("tree-drop")));
                let text = match (&target, verdict) {
                    (Some((r, into, _)), Some(Ok(()))) => {
                        // the whole list stands for the top of the vault: no box around it
                        if !into.as_os_str().is_empty() {
                            painter.rect_stroke(*r, CornerRadius::same(5), Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
                        }
                        format!("Move “{name}” into {}", if into.as_os_str().is_empty() { "the top of the vault".to_string() } else { format!("“{}”", rel_to_string(into)) })
                    }
                    (_, Some(Err(why))) => format!("“{name}”: {why}"),
                    _ => format!("Move “{name}”: drop it on a folder"),
                };
                self.paint_hint_text(&ctx, p, &text);
                ctx.request_repaint();
            }
        }
        // an event dragged out of the calendar
        let cal = egui::DragAndDrop::payload::<crate::calendar::ui::CalDrag>(&ctx);
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        if let (Some(drag), Some(p)) = (&cal, pointer) {
            let over_calendar = self.drag.calendar_areas.iter().any(|r| r.contains(p));
            let zone = if over_calendar { None } else { self.drag.zone_at(p) };
            if ctx.input(|i| i.pointer.any_released()) {
                egui::DragAndDrop::clear_payload(&ctx);
                if !over_calendar {
                    let file = format!("{}.ics", if drag.event.summary.trim().is_empty() { "event".to_string() } else { drag.event.summary.replace(['/', '\\'], "-") });
                    let item = import::event_item(drag.event.clone(), drag.calendar.clone(), Some((file, crate::calendar::ics::to_ics("", std::slice::from_ref(&drag.event)).into_bytes())));
                    self.open_import(vec![item], vec![], zone.map(|z| (z.folder, z.note)));
                }
            } else if over_calendar {
                self.paint_hint_text(&ctx, p, &format!("Event: {}\ndrag it onto a note, a folder or the editor to import it", drag.event.summary));
            } else {
                let label = format!("Event: {}", if drag.event.summary.is_empty() { "(no title)" } else { &drag.event.summary });
                self.paint_drop_hint(&ctx, p, zone.as_ref(), &label);
            }
        }
        let ext = self.drag.ext_hover.or(if hovering_files { pointer } else { None });
        if let Some(p) = ext {
            let zone = self.drag.zone_at(p);
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

fn pointer_now(ctx: &egui::Context) -> Option<Pos2> {
    ctx.input(|i| i.pointer.latest_pos())
}
