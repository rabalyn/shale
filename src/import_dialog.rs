//! The "Import" dialog shown when e-mails, events or files are dropped on the window (or
//! chosen with the Import button): what to import, the folder, tags and a few options.

use crate::import::{self, Item, Source};
use crate::theme;
use eframe::egui::{self, Rect, Ui};
use std::path::PathBuf;

pub struct ImportDialog {
    pub items: Vec<Item>,
    /// vault folder for the new notes
    pub folder: String,
    pub tags: String,
    pub keep_original: bool,
    /// a note to add links to the new notes to (where it was dropped / the open note)
    pub link_into: Option<PathBuf>,
    pub link_on: bool,
    /// typed file path to add
    pub add_path: String,
    /// things that could not be taken (shown in the dialog)
    pub notes: Vec<String>,
    pub error: Option<String>,
    pub rects: Vec<(String, Rect)>,
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
        ImportDialog { items, folder, tags, keep_original: true, link_into, link_on, add_path: String::new(), notes, error: None, rects: vec![] }
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

fn size_text(n: usize) -> String {
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
    ui.set_width(620.0);
    ui.label(egui::RichText::new("Import into the vault").size(18.0).color(theme::TEXT_STRONG));
    ui.add_space(4.0);
    // ---- what
    if d.items.is_empty() {
        ui.label(muted("Nothing to import yet. Drop e-mails, calendar events or files on the window, or add a file below."));
    }
    let max_h = (ui.ctx().content_rect().height() - 430.0).max(140.0);
    egui::ScrollArea::vertical().id_salt("import-items").max_height(max_h).show(ui, |ui| {
        for (i, it) in d.items.iter_mut().enumerate() {
            egui::Frame::new().fill(theme::BG_SECONDARY).corner_radius(6).inner_margin(8).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    rec(format!("include:{i}"), ui.checkbox(&mut it.include, "")).on_hover_text("Import this one");
                    ui.label(egui::RichText::new(it.kind()).size(11.0).color(theme::ACCENT));
                    rec(format!("title:{i}"), ui.add(egui::TextEdit::singleline(&mut it.title).desired_width(470.0))).on_hover_text("Name of the new note");
                });
                ui.label(muted(&it.detail()));
                if let Some(p) = &it.already {
                    ui.label(egui::RichText::new(format!("Imported before as “{}”. Tick it to import it again.", crate::markdown::strip_md_ext(&p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))).size(12.0).color(theme::LINK_MISSING));
                }
                if let Source::Email { mail, .. } = &it.source {
                    for (k, (a, on)) in mail.attachments.iter().zip(it.att_on.iter_mut()).enumerate() {
                        let label = format!("{}  ({}, {}{})", a.name, a.mime, size_text(a.data.len()), if a.inline { ", in the text" } else { "" });
                        rec(format!("att:{i}:{k}"), ui.checkbox(on, egui::RichText::new(label).size(12.5)));
                    }
                }
            });
            ui.add_space(4.0);
        }
    });
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
    });
    rec("keep".into(), ui.checkbox(&mut d.keep_original, "Keep the original message / calendar file with the attachments"));
    if let Some(n) = &d.link_into {
        let name = crate::markdown::strip_md_ext(&n.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()).to_string();
        rec("link".into(), ui.checkbox(&mut d.link_on, format!("Add links to the new notes in “{name}”")));
    }
    if let Some(e) = &d.error {
        ui.add(egui::Label::new(egui::RichText::new(e).color(theme::LINK_MISSING)).wrap());
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let n = d.count();
        let label = if n == 1 { "Import 1 item".to_string() } else { format!("Import {n} items") };
        if rec("import".into(), ui.add_enabled(n > 0, egui::Button::new(egui::RichText::new(label).strong()))).clicked() {
            action = Some(ImportAction::Import);
        }
        if rec("cancel".into(), ui.button("Cancel")).clicked() {
            action = Some(ImportAction::Cancel);
        }
    });
    ui.label(muted("Drag messages from your mail program, events from a calendar (also Shale's own), or any files onto the window to add more."));
    d.rects = rects;
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
                    res = show(ui, d, folders, tags);
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

    #[test]
    fn every_control_of_the_import_dialog() {
        let (items, _) = import::items_from_drop("message/rfc822", &crate::mail::tests::sample_eml());
        let (more, _) = import::items_from_drop("text/plain", b"Notiz");
        let mut all = items;
        all.extend(more);
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
        h.click("keep", &mut d, &folders, &tags);
        assert!(!d.keep_original);
        h.click("link", &mut d, &folders, &tags);
        assert!(d.link_on);
        h.typ("add-path", "~/x.eml", &mut d, &folders, &tags);
        assert_eq!(h.click("add", &mut d, &folders, &tags), Some(ImportAction::AddPath));
        assert_eq!(h.click("import", &mut d, &folders, &tags), Some(ImportAction::Import));
        assert_eq!(h.click("cancel", &mut d, &folders, &tags), Some(ImportAction::Cancel));
    }
}
