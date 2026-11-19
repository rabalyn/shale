//! The "Import" dialog shown when e-mails, events or files are dropped on the window (or
//! chosen with the Import button): what to import, the folder, tags and a few options.

use crate::import::{self, docx, Item, Source};
use crate::theme;
use eframe::egui::{self, Rect, Ui};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;

/// The result of pandoc for the item at this place of the list.
pub type Converted = (usize, Result<Arc<docx::Prepared>, String>);

pub struct ImportDialog {
    pub items: Vec<Item>,
    /// vault folder for the new notes
    pub folder: String,
    pub tags: String,
    pub keep_original: bool,
    /// keep the original .docx of a Word file in its import folder (IMP-038)
    pub keep_docx: bool,
    /// also write the header of an e-mail / event as a list in the note (IMP-052)
    pub header_list: bool,
    /// pandoc runs for the Word files that are going on in a job (IMP-035)
    pub converting: Option<Receiver<Result<Vec<Converted>, String>>>,
    /// set when the dialog is let go: a pandoc run that is still going stops
    pub cancel: Arc<AtomicBool>,
    /// a note to add links to the new notes to (where it was dropped / the open note)
    pub link_into: Option<PathBuf>,
    pub link_on: bool,
    /// typed file path to add
    pub add_path: String,
    /// things that could not be taken (shown in the dialog)
    pub notes: Vec<String>,
    pub error: Option<String>,
    pub rects: Vec<(String, Rect)>,
    /// tags found written in the items (put into `tags` automatically)
    pub auto_tags: Vec<String>,
    /// learned suggestions: (tag, the words that point to it)
    pub suggested: Vec<(String, Vec<String>)>,
    /// the people folder (shown in the people list)
    pub people_dir: String,
}

#[derive(Debug, PartialEq)]
pub enum ImportAction {
    Import,
    Cancel,
    /// add the file typed in `add_path`
    AddPath,
}

impl ImportDialog {
    pub fn new(items: Vec<Item>, notes: Vec<String>, folder: String, tags: String, link_into: Option<PathBuf>, link_on: bool) -> Self {
        ImportDialog {
            items,
            folder,
            tags,
            keep_original: true,
            keep_docx: true,
            header_list: false,
            converting: None,
            cancel: Arc::new(AtomicBool::new(false)),
            link_into,
            link_on,
            add_path: String::new(),
            notes,
            error: None,
            rects: vec![],
            auto_tags: vec![],
            suggested: vec![],
            people_dir: crate::import::people::DEFAULT_DIR.into(),
        }
    }

    /// Start pandoc, in a job, on the Word files that have not been converted yet (IMP-035). False
    /// when there is nothing to do.
    pub fn start_conversion(&mut self, root: PathBuf) -> bool {
        let todo: Vec<(usize, PathBuf, PathBuf)> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.include)
            .filter_map(|(k, i)| match &i.source {
                Source::Docx { path, pandoc: Some(p), ready: None, .. } => Some((k, p.clone(), path.clone())),
                _ => None,
            })
            .collect();
        if todo.is_empty() {
            return false;
        }
        let (keep, cancel, date) = (self.keep_docx, self.cancel.clone(), crate::media::local_date());
        self.converting = Some(crate::jobs::spawn(
            move || {
                let mut reserved = std::collections::HashSet::new();
                todo.into_iter()
                    .map(|(k, pandoc, path)| {
                        let c = docx::Convert { pandoc: &pandoc, docx: &path, root: &root, date, keep_original: keep, timeout: docx::TIMEOUT, cancel: &cancel };
                        (k, docx::prepare(&c, &mut reserved).map(Arc::new))
                    })
                    .collect()
            },
            || {},
        ));
        true
    }

    /// Take the result of the job when it has ended. True when it has: the import can go on.
    pub fn poll_conversion(&mut self) -> bool {
        let Some(rx) = &self.converting else { return false };
        let done = match rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Err("the conversion stopped without a result".into()),
        };
        self.converting = None;
        match done {
            Ok(results) => {
                for (k, r) in results {
                    if let Some(Source::Docx { ready, .. }) = self.items.get_mut(k).map(|i| &mut i.source) {
                        *ready = Some(r);
                    }
                }
            }
            Err(e) => {
                // a crash: every file waiting for it fails with its words
                for it in &mut self.items {
                    if let Source::Docx { ready: ready @ None, pandoc: Some(_), .. } = &mut it.source {
                        *ready = Some(Err(e.clone()));
                    }
                }
            }
        }
        true
    }

    /// Forget the results of pandoc, so that the next Import runs it again (after a failure).
    pub fn forget_conversions(&mut self) {
        for it in &mut self.items {
            if let Source::Docx { ready, .. } = &mut it.source {
                *ready = None;
            }
        }
    }

    pub fn count(&self) -> usize {
        self.items.iter().filter(|i| i.include).count()
    }

    /// Put a tag in the tag field, or take it out if it is there.
    pub fn toggle_tag(&mut self, tag: &str) {
        let mut tags = import::parse_tags(&self.tags);
        match tags.iter().position(|t| t.eq_ignore_ascii_case(tag)) {
            Some(i) => {
                tags.remove(i);
            }
            None => tags.push(tag.to_string()),
        }
        self.tags = tags.join(", ");
    }
}

impl Drop for ImportDialog {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

pub(crate) fn size_text(n: usize) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1 << 20) as f64)
    } else {
        format!("{} KB", n.div_ceil(1024))
    }
}

/// `folders`: the vault's folders; `tag_choices`: recently used and common tags.
pub fn show(ui: &mut Ui, d: &mut ImportDialog, folders: &[String], tag_choices: &[String]) -> Option<ImportAction> {
    let mut rects: Vec<(String, Rect)> = Vec::new();
    let mut rec = |k: String, r: egui::Response| -> egui::Response {
        rects.push((k, r.rect));
        r
    };
    let mut action = None;
    let muted = |t: &str| egui::RichText::new(t).size(12.0).color(theme::MUTED);
    // ---- what
    if d.items.is_empty() {
        crate::theme::prose(ui, muted("Nothing to import yet. Drop e-mails, calendar events, pictures or other files on the window, or add a file below."));
    }
    for (i, it) in d.items.iter_mut().enumerate() {
        egui::Frame::new().fill(theme::BG_SECONDARY).corner_radius(6).inner_margin(8).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                rec(format!("include:{i}"), ui.checkbox(&mut it.include, "")).on_hover_text("Import this one");
                ui.label(egui::RichText::new(it.kind()).size(11.0).color(theme::ACCENT));
                let tip = if matches!(it.source, Source::Media { .. }) { "File name in the media folder" } else { "Name of the new note" };
                rec(format!("title:{i}"), ui.add(egui::TextEdit::singleline(&mut it.title).desired_width(470.0))).on_hover_text(tip);
            });
            ui.label(muted(&it.detail()));
            if let Some(p) = &it.already {
                ui.label(
                    egui::RichText::new(format!("Imported before as “{}”. Tick it to import it again.", crate::markdown::strip_md_ext(&p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())))
                        .size(12.0)
                        .color(theme::LINK_MISSING),
                );
            }
            if let Some(w) = it.warning() {
                ui.add(egui::Label::new(egui::RichText::new(w).size(12.0).color(theme::LINK_MISSING)).wrap());
            }
            if !it.people.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 2.0);
                    ui.label(egui::RichText::new("People").size(12.0).color(theme::MUTED)).on_hover_text("Ticked people are linked from the note; those without a contact note get one in the people folder");
                    for (k, p) in it.people.iter_mut().enumerate() {
                        let what = if p.existing.is_some() { "" } else { " (new)" };
                        let label = egui::RichText::new(format!("{}{what}", p.name)).size(12.5).color(if p.existing.is_some() { theme::LINK } else { theme::TEXT });
                        let tip = format!("{} · {}{}", p.role.label(), if p.email.is_empty() { "no address" } else { p.email.as_str() }, if p.existing.is_some() { " · has a contact note" } else { " · a contact note will be made" });
                        rec(format!("person:{i}:{k}"), ui.checkbox(&mut p.on, label)).on_hover_text(tip);
                    }
                });
            }
            if let Source::Markdown { text, lift, .. } = &mut it.source {
                if crate::markdown::lift_headings(text).is_some() {
                    rec(format!("lift:{i}"), ui.checkbox(lift, egui::RichText::new("Lift headings (## → #, ### → ##, …)").size(12.5)))
                        .on_hover_text("The text has no # heading: every heading moves up by the same number of levels so that the highest becomes #");
                }
            }
            if let Source::Docx { lift, pandoc: Some(_), .. } = &mut it.source {
                rec(format!("lift:{i}"), ui.checkbox(lift, egui::RichText::new("Lift headings (## → #, ### → ##, …)").size(12.5)))
                    .on_hover_text("When the document has no # heading: every heading moves up by the same number of levels so that the highest becomes #");
            }
            if let Source::Email { mail, .. } = &it.source {
                for (k, (a, on)) in mail.attachments.iter().zip(it.att_on.iter_mut()).enumerate() {
                    let label = format!("{}  ({}, {}{})", a.name, a.mime, size_text(a.data.len()), if a.inline { ", in the text" } else { "" });
                    rec(format!("att:{i}:{k}"), ui.checkbox(on, egui::RichText::new(label).size(12.5)));
                }
            }
            if let Source::Social { saved, replies, .. } = &mut it.source {
                if !saved.replies.is_empty() {
                    rec(format!("replies:{i}"), ui.checkbox(replies, egui::RichText::new(format!("Include replies from others ({})", saved.replies.len())).size(12.5)))
                        .on_hover_text("Adds a Replies section with other people's answers to the thread, nested; their pictures are only linked");
                }
                for (k, (m, on)) in saved.media.iter().zip(it.att_on.iter_mut()).enumerate() {
                    let label = format!("{}  ({})", m.file_name, size_text(m.data.len()));
                    rec(format!("att:{i}:{k}"), ui.checkbox(on, egui::RichText::new(label).size(12.5))).on_hover_text("Untick to leave this file out; the note then links to its web address");
                }
            }
        });
        ui.add_space(4.0);
    }
    for n in &d.notes {
        ui.add(egui::Label::new(egui::RichText::new(n).size(12.0).color(theme::LINK_MISSING)).wrap());
    }
    ui.horizontal(|ui| {
        rec("add-path".into(), ui.add(egui::TextEdit::singleline(&mut d.add_path).hint_text("~/Downloads/message.eml").desired_width(420.0)));
        if rec("add".into(), ui.add_enabled(!d.add_path.trim().is_empty(), egui::Button::new("Add file"))).on_hover_text("Add an .eml, .ics or any other file").clicked() {
            action = Some(ImportAction::AddPath);
        }
    });
    ui.separator();
    // ---- where and how
    egui::Grid::new("import-opts").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
        ui.label(muted("Folder"));
        ui.horizontal(|ui| {
            rec("folder".into(), ui.add(egui::TextEdit::singleline(&mut d.folder).hint_text("Projects/New lecture hall").desired_width(380.0))).on_hover_text("A new folder is made if needed");
            let combo = egui::ComboBox::from_id_salt("import-folder").selected_text("Pick…").width(90.0).show_ui(ui, |ui| {
                if rec("folder-pick:/".into(), ui.selectable_label(d.folder.is_empty(), "(vault root)")).clicked() {
                    d.folder.clear();
                }
                for f in folders {
                    if rec(format!("folder-pick:{f}"), ui.selectable_label(d.folder == *f, f)).clicked() {
                        d.folder = f.clone();
                    }
                }
            });
            rec("folder-combo".into(), combo.response);
        });
        ui.end_row();
        ui.label(muted("Tags"));
        ui.vertical(|ui| {
            rec("tags".into(), ui.add(egui::TextEdit::singleline(&mut d.tags).hint_text("e.g. lecture-hall-b, projector, status").desired_width(480.0)));
            if !tag_choices.is_empty() {
                let current = import::parse_tags(&d.tags);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                    for t in tag_choices.iter().take(24) {
                        let on = current.iter().any(|c| c.eq_ignore_ascii_case(t));
                        if rec(format!("tag:{t}"), ui.selectable_label(on, egui::RichText::new(format!("#{t}")).size(12.0))).clicked() {
                            d.toggle_tag(t);
                        }
                    }
                });
            }
        });
        ui.end_row();
        if !d.suggested.is_empty() || !d.auto_tags.is_empty() {
            ui.label(muted("Suggested"));
            ui.vertical(|ui| {
                if !d.auto_tags.is_empty() {
                    ui.label(egui::RichText::new(format!("Added because they appear in the text: {}", d.auto_tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" "))).size(11.5).color(theme::FAINT));
                }
                let current = import::parse_tags(&d.tags);
                let mut add = None;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                    for (t, why) in &d.suggested {
                        let on = current.iter().any(|c| c.eq_ignore_ascii_case(t));
                        if rec(format!("suggest:{t}"), ui.selectable_label(on, egui::RichText::new(format!("#{t}")).size(12.0))).on_hover_text(format!("Notes with this tag often mention: {}", why.join(", "))).clicked() {
                            add = Some(t.clone());
                        }
                    }
                });
                if let Some(t) = add {
                    d.toggle_tag(&t);
                }
            });
            ui.end_row();
        }
    });
    if d.items.iter().any(|i| matches!(i.source, Source::Email { .. } | Source::Event { .. })) {
        rec("keep".into(), ui.checkbox(&mut d.keep_original, "Keep the original message / calendar file with the attachments"));
        rec("header-list".into(), ui.checkbox(&mut d.header_list, "Header list in the note body"))
            .on_hover_text("From, To, Date, When, Where … are always in the note's properties; this also writes them as a list below the title, for programs that do not show properties");
    }
    if d.items.iter().any(|i| matches!(i.source, Source::Docx { pandoc: Some(_), .. })) {
        rec("keep-docx".into(), ui.checkbox(&mut d.keep_docx, "Keep the original .docx")).on_hover_text("Copies each Word file into its import folder and names it in the note's properties");
    }
    if d.converting.is_some() {
        ui.label(muted("Converting the Word files with pandoc…"));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    let included = || d.items.iter().filter(|i| i.include);
    let media = included().filter(|i| matches!(i.source, Source::Media { .. })).count();
    if media > 0 {
        let what = if media == included().count() {
            "Pictures, PDFs and audio files are saved as files in the media folder (no note, no folder or tags)."
        } else {
            "Pictures, PDFs and audio files are saved as files in the media folder; folder and tags are for the notes."
        };
        ui.label(muted(what));
    }
    if let Some(n) = &d.link_into {
        let name = crate::markdown::strip_md_ext(&n.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()).to_string();
        let new = if media == 0 {
            "notes"
        } else if media == included().count() {
            "files"
        } else {
            "notes and files"
        };
        rec("link".into(), ui.checkbox(&mut d.link_on, format!("Add links to the new {new} in “{name}”")));
    }
    crate::theme::prose(ui, muted("Drag messages from your mail program, events from a calendar (also Shale's own), or any files onto the window to add more."));
    d.rects = rects;
    action
}

/// The dialog's footer (NAV-028): the error of the last try and the buttons, below the
/// scrolling content. Call it after [`show`] in the same frame.
pub fn footer(ui: &mut Ui, d: &mut ImportDialog) -> Option<ImportAction> {
    let mut action = None;
    if let Some(e) = &d.error {
        ui.add(egui::Label::new(egui::RichText::new(e).color(theme::LINK_MISSING)).wrap());
    }
    let n = d.count();
    let mut rec = |k: &str, r: egui::Response| -> egui::Response {
        d.rects.push((k.to_string(), r.rect));
        r
    };
    ui.horizontal(|ui| {
        let label = if n == 1 { "Import 1 item".to_string() } else { format!("Import {n} items") };
        if rec("import", ui.add_enabled(n > 0 && d.converting.is_none(), egui::Button::new(egui::RichText::new(label).strong()))).clicked() {
            action = Some(ImportAction::Import);
        }
        if rec("cancel", ui.button("Cancel")).clicked() {
            action = Some(ImportAction::Cancel);
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event as Ev, Modifiers, PointerButton, Pos2, RawInput, Vec2};

    struct H {
        ctx: egui::Context,
        t: f64,
    }
    impl H {
        fn frame(&mut self, events: Vec<Ev>, d: &mut ImportDialog, folders: &[String], tags: &[String]) -> Option<ImportAction> {
            self.t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 1000.0))), time: Some(self.t), events, ..Default::default() };
            let mut res = None;
            let mut out = self.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let body = show(ui, d, folders, tags);
                    res = footer(ui, d).or(body);
                });
            });
            out.textures_delta.clear();
            res
        }
        fn click(&mut self, key: &str, d: &mut ImportDialog, folders: &[String], tags: &[String]) -> Option<ImportAction> {
            let p = d.rects.iter().find(|r| r.0 == key).unwrap_or_else(|| panic!("no {key}: {:?}", d.rects.iter().map(|r| &r.0).collect::<Vec<_>>())).1.center();
            let b = |pressed| Ev::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            let mut r = self.frame(vec![Ev::PointerMoved(p)], d, folders, tags);
            r = self.frame(vec![b(true)], d, folders, tags).or(r);
            r = self.frame(vec![b(false)], d, folders, tags).or(r);
            self.frame(vec![], d, folders, tags).or(r)
        }
        fn typ(&mut self, key: &str, text: &str, d: &mut ImportDialog, folders: &[String], tags: &[String]) {
            self.click(key, d, folders, tags);
            let end = Ev::Key { key: egui::Key::End, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
            self.frame(vec![end], d, folders, tags);
            self.frame(vec![Ev::Text(text.into())], d, folders, tags);
        }
    }

    // spec: IMP-001
    #[test]
    fn every_control_of_the_import_dialog() {
        // a second item: dropped text where a program can drop it (Wayland), else a second message
        let mut all = vec![import::eml_item(crate::mail::tests::sample_eml(), None)];
        #[cfg(target_os = "linux")]
        all.extend(import::items_from_drop("text/plain", b"Notiz").0);
        #[cfg(not(target_os = "linux"))]
        all.push(import::eml_item(crate::mail::tests::sample_eml(), None));
        let mut d = ImportDialog::new(all, vec!["“x” is a folder; drop the files inside it instead.".into()], String::new(), String::new(), Some(PathBuf::from("Projekte/Übersicht.md")), false);
        let folders = vec!["Projekte".to_string(), "Projekte/Neubau B".to_string()];
        let tags = vec!["neubau-b".to_string(), "status".to_string()];
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        h.frame(vec![], &mut d, &folders, &tags);
        assert_eq!(d.count(), 2);
        h.click("include:1", &mut d, &folders, &tags);
        assert_eq!(d.count(), 1, "untick an item");
        assert!(!d.items[0].att_on[0]);
        h.click("att:0:0", &mut d, &folders, &tags);
        assert!(d.items[0].att_on[0], "attachment checkbox");
        h.click("att:0:1", &mut d, &folders, &tags);
        assert!(!d.items[0].att_on[1]);
        h.typ("title:0", " (Kopie)", &mut d, &folders, &tags);
        assert!(d.items[0].title.ends_with("(Kopie)"), "{}", d.items[0].title);
        h.click("folder-combo", &mut d, &folders, &tags);
        h.click("folder-pick:Projekte/Neubau B", &mut d, &folders, &tags);
        assert_eq!(d.folder, "Projekte/Neubau B");
        h.typ("tags", "beamer", &mut d, &folders, &tags);
        h.click("tag:status", &mut d, &folders, &tags);
        assert_eq!(import::parse_tags(&d.tags), vec!["beamer", "status"]);
        h.click("tag:status", &mut d, &folders, &tags);
        assert_eq!(import::parse_tags(&d.tags), vec!["beamer"], "clicking a tag again takes it out");
        // people and suggested tags
        d.items[0].people = vec![crate::import::people::Person { name: "Hans Müller".into(), email: "h@x.de".into(), role: crate::import::people::Role::From, existing: None, on: true }];
        d.suggested = vec![("av-technik".into(), vec!["leinwand".into(), "projektor".into()])];
        h.frame(vec![], &mut d, &folders, &tags);
        h.click("person:0:0", &mut d, &folders, &tags);
        assert!(!d.items[0].people[0].on, "a person can be left out");
        h.click("suggest:av-technik", &mut d, &folders, &tags);
        assert_eq!(import::parse_tags(&d.tags), vec!["beamer", "av-technik"], "a suggestion is added with a click");
        h.click("keep", &mut d, &folders, &tags);
        assert!(!d.keep_original);
        h.click("link", &mut d, &folders, &tags);
        assert!(d.link_on);
        h.typ("add-path", "~/x.eml", &mut d, &folders, &tags);
        assert_eq!(h.click("add", &mut d, &folders, &tags), Some(ImportAction::AddPath));
        assert_eq!(h.click("import", &mut d, &folders, &tags), Some(ImportAction::Import));
        assert_eq!(h.click("cancel", &mut d, &folders, &tags), Some(ImportAction::Cancel));
    }

    // spec: IMP-052
    #[test]
    fn the_header_list_box_is_off_and_only_there_for_messages_and_events() {
        let mut d = ImportDialog::new(vec![import::eml_item(crate::mail::tests::sample_eml(), None)], vec![], String::new(), String::new(), None, false);
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        h.frame(vec![], &mut d, &[], &[]);
        assert!(!d.header_list, "off at first");
        h.click("header-list", &mut d, &[], &[]);
        assert!(d.header_list);
        h.click("header-list", &mut d, &[], &[]);
        assert!(!d.header_list);
        let mut d = ImportDialog::new(vec![import::markdown_item("Top.md".into(), "# A\n".into())], vec![], String::new(), String::new(), None, false);
        h.frame(vec![], &mut d, &[], &[]);
        assert!(!d.rects.iter().any(|r| r.0 == "header-list"), "nothing to list for a Markdown note");
    }

    // spec: IMP-034
    #[test]
    fn the_lift_box_is_there_for_a_markdown_note_without_h1_and_ticked() {
        let items = vec![import::markdown_item("Deep.md".into(), "## A\n### B\n".into()), import::markdown_item("Top.md".into(), "# A\n".into()), import::markdown_item("Plain.md".into(), "text".into())];
        let mut d = ImportDialog::new(items, vec![], String::new(), String::new(), None, false);
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        h.frame(vec![], &mut d, &[], &[]);
        assert!(d.rects.iter().any(|r| r.0 == "lift:0"));
        assert!(!d.rects.iter().any(|r| r.0 == "lift:1" || r.0 == "lift:2"), "no box without headings or with a #");
        assert!(matches!(d.items[0].source, Source::Markdown { lift: true, .. }), "ticked");
        h.click("lift:0", &mut d, &[], &[]);
        assert!(matches!(d.items[0].source, Source::Markdown { lift: false, .. }), "a click unticks it");
        h.click("lift:0", &mut d, &[], &[]);
        assert!(matches!(d.items[0].source, Source::Markdown { lift: true, .. }));
    }

    // spec: IMP-035, IMP-038
    #[test]
    fn a_word_item_has_the_lift_and_keep_boxes_and_one_without_pandoc_has_none() {
        let with = import::docx::docx_item(PathBuf::from("/in/Report.docx"), Some(PathBuf::from("/usr/bin/pandoc")));
        let without = import::docx::docx_item(PathBuf::from("/in/Other.docx"), None);
        assert_eq!((with.kind().as_ref(), without.kind().as_ref()), ("Word document", "Files"));
        let mut d = ImportDialog::new(vec![with, without], vec![], String::new(), String::new(), None, false);
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        h.frame(vec![], &mut d, &[], &[]);
        assert!(d.rects.iter().any(|r| r.0 == "lift:0") && d.rects.iter().any(|r| r.0 == "keep-docx"));
        assert!(!d.rects.iter().any(|r| r.0 == "lift:1"), "an item without pandoc is only an attachment");
        assert!(d.keep_docx && matches!(d.items[0].source, Source::Docx { lift: true, .. }), "both ticked at first");
        h.click("keep-docx", &mut d, &[], &[]);
        assert!(!d.keep_docx);
        h.click("lift:0", &mut d, &[], &[]);
        assert!(matches!(d.items[0].source, Source::Docx { lift: false, .. }));
        // no Word file with pandoc, no box
        let mut d = ImportDialog::new(vec![import::docx::docx_item(PathBuf::from("/in/Other.docx"), None)], vec![], String::new(), String::new(), None, false);
        h.frame(vec![], &mut d, &[], &[]);
        assert!(!d.rects.iter().any(|r| r.0 == "keep-docx"));
    }

    // spec: IMP-035
    #[test]
    fn the_result_of_the_conversion_job_is_taken_and_a_crash_fails_the_files_waiting_for_it() {
        let ready = |d: &ImportDialog| -> Vec<Option<bool>> { d.items.iter().map(|i| if let Source::Docx { ready, .. } = &i.source { ready.as_ref().map(|r| r.is_ok()) } else { None }).collect() };
        let item = || import::docx::docx_item(PathBuf::from("/in/a.docx"), Some(PathBuf::from("/no/such/pandoc")));
        let mut d = ImportDialog::new(vec![item(), import::markdown_item("n.md".into(), "x".into())], vec![], String::new(), String::new(), None, false);
        assert!(!d.poll_conversion(), "nothing is running");
        // the job runs pandoc, which is not there: the file fails with the words for it
        assert!(d.start_conversion(std::env::temp_dir().join("shale-dialog-conv-never-used")));
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !d.poll_conversion() {
            assert!(std::time::Instant::now() < end);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(ready(&d), vec![Some(false), None]);
        assert!(!d.start_conversion(std::env::temp_dir()), "what has a result is not run again");
        d.forget_conversions();
        assert_eq!(ready(&d), vec![None, None]);
        // a job that ends without a result
        let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<Converted>, String>>();
        drop(tx);
        d.converting = Some(rx);
        assert!(d.poll_conversion());
        match &d.items[0].source {
            Source::Docx { ready: Some(Err(e)), .. } => assert!(e.contains("without a result"), "{e}"),
            other => panic!("{other:?}"),
        }
        // a job that crashed
        d.forget_conversions();
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Err("the background job crashed: boom".to_string())).unwrap();
        d.converting = Some(rx);
        assert!(d.poll_conversion());
        assert!(matches!(&d.items[0].source, Source::Docx { ready: Some(Err(e)), .. } if e.contains("boom")));
        // letting go of the dialog stops a run that is still going
        let flag = d.cancel.clone();
        drop(d);
        assert!(flag.load(Ordering::SeqCst));
    }

    // spec: SOC-020
    #[test]
    fn the_replies_option_is_off_by_default_and_only_there_when_there_are_replies() {
        use crate::social::demo;
        let posts = demo::posts();
        let items = vec![import::social_item(demo::saved(&posts[4]), "Mastodon", false), import::social_item(demo::saved(&posts[3]), "Mastodon", false)];
        let mut d = ImportDialog::new(items, vec![], "Social".into(), String::new(), None, false);
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        h.frame(vec![], &mut d, &[], &[]);
        assert!(d.rects.iter().any(|r| r.0 == "replies:0"));
        assert!(!d.rects.iter().any(|r| r.0 == "replies:1"), "no replies, no option");
        assert!(matches!(d.items[0].source, Source::Social { replies: false, .. }));
        h.click("replies:0", &mut d, &[], &[]);
        assert!(matches!(d.items[0].source, Source::Social { replies: true, .. }));
        // a remembered "on" applies only to posts that have replies
        assert!(matches!(import::social_item(demo::saved(&posts[3]), "Mastodon", true).source, Source::Social { replies: false, .. }));
    }
}
