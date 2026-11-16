//! The "Export to PDF" dialog: paper size, orientation, margins, text size, what to include,
//! background and scale for drawings, where to save, and whether to open the PDF afterwards.

use crate::pdf::export::{Background, ExportOptions, Paper};
use crate::theme;
use eframe::egui::{self, Rect, Ui};
use std::path::PathBuf;

pub struct ExportDialog {
    pub rel: PathBuf,
    pub drawing: bool,
    pub name: String,
    pub opts: ExportOptions,
    pub path: String,
    pub open_after: bool,
    pub error: Option<String>,
    /// screen rect of every control (used by the UI tests)
    pub rects: Vec<(String, Rect)>,
}

#[derive(PartialEq, Eq, Debug)]
pub enum DialogResult {
    Open,
    Export,
    Cancel,
}

impl ExportDialog {
    pub fn new(rel: PathBuf, drawing: bool, name: String, opts: ExportOptions, default_path: PathBuf) -> Self {
        ExportDialog { rel, drawing, name, opts, path: default_path.to_string_lossy().into_owned(), open_after: false, error: None, rects: vec![] }
    }
}

pub fn show(ui: &mut Ui, d: &mut ExportDialog) {
    d.rects.clear();
    let mut rects: Vec<(String, Rect)> = Vec::new();
    let mut rec = |k: &str, r: egui::Response| -> egui::Response {
        rects.push((k.to_string(), r.rect));
        r
    };
    let muted = |t: &str| egui::RichText::new(t).size(12.0).color(theme::MUTED);
    egui::Grid::new("export-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.label(muted("Paper"));
        let choices: &[Paper] = if d.drawing { &Paper::DRAWING } else { &Paper::NOTE };
        let combo = egui::ComboBox::from_id_salt("export-paper").width(250.0).selected_text(d.opts.paper.label()).show_ui(ui, |ui| {
            for p in choices {
                if rec(&format!("paper:{}", p.label()), ui.selectable_label(d.opts.paper == *p, p.label())).clicked() {
                    d.opts.paper = *p;
                }
            }
        });
        rec("paper", combo.response);
        ui.end_row();

        let fit = d.opts.paper == Paper::FitDrawing;
        ui.label(muted("Orientation"));
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!fit, |ui| {
                if rec("portrait", ui.selectable_label(!d.opts.landscape, "Portrait")).clicked() {
                    d.opts.landscape = false;
                }
                if rec("landscape", ui.selectable_label(d.opts.landscape, "Landscape")).clicked() {
                    d.opts.landscape = true;
                }
            });
        });
        ui.end_row();

        ui.label(muted(if fit { "Border" } else { "Margins" }));
        rec("margin", ui.add(egui::Slider::new(&mut d.opts.margin_mm, 0.0..=40.0).suffix(" mm").step_by(1.0)));
        ui.end_row();

        if d.drawing {
            ui.label(muted("Background"));
            ui.horizontal(|ui| {
                for (b, t) in [(Background::White, "White"), (Background::Dark, "Dark"), (Background::None, "Transparent")] {
                    if rec(&format!("bg:{t}"), ui.selectable_label(d.opts.background == b, t)).clicked() {
                        d.opts.background = b;
                    }
                }
            });
            ui.end_row();
            ui.label(muted("Size"));
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!fit, |ui| {
                    if rec("scale:fill", ui.selectable_label(d.opts.fill_page, "Fill the page")).clicked() {
                        d.opts.fill_page = true;
                    }
                    if rec("scale:actual", ui.selectable_label(!d.opts.fill_page, "Actual size")).clicked() {
                        d.opts.fill_page = false;
                    }
                });
            });
            ui.end_row();
        } else {
            ui.label(muted("Text size"));
            rec("font", ui.add(egui::Slider::new(&mut d.opts.font_pt, 8.0..=18.0).suffix(" pt").step_by(0.5)));
            ui.end_row();
            ui.label(muted("Include"));
            ui.vertical(|ui| {
                rec("title", ui.checkbox(&mut d.opts.title, "The note's name as a title"));
                rec("pagenumbers", ui.checkbox(&mut d.opts.page_numbers, "Page numbers"));
                rec("embeds", ui.checkbox(&mut d.opts.embeds, "Embedded pictures, drawings and PDFs"));
                rec("plain-tables", ui.checkbox(&mut d.opts.plain_tables, "Export plain text (tables as raw text lines)"));
            });
            ui.end_row();
        }

        ui.label(muted("Save as"));
        rec("path", ui.add(egui::TextEdit::singleline(&mut d.path).desired_width(250.0)));
        ui.end_row();
    });
    ui.add_space(4.0);
    rec("open-after", ui.checkbox(&mut d.open_after, "Open the PDF after exporting"));
    d.rects = rects;
}

/// The dialog's footer (NAV-028): the error of the last try, Export and Cancel. Call it after
/// [`show`] in the same frame.
pub fn footer(ui: &mut Ui, d: &mut ExportDialog) -> DialogResult {
    let mut result = DialogResult::Open;
    if let Some(e) = &d.error {
        ui.colored_label(theme::LINK_MISSING, e);
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let mut rec = |k: &str, r: egui::Response| -> egui::Response {
            d.rects.push((k.to_string(), r.rect));
            r
        };
        if rec("export", ui.button(egui::RichText::new("Export").strong())).clicked() {
            result = DialogResult::Export;
        }
        if rec("cancel", ui.button("Cancel")).clicked() {
            result = DialogResult::Cancel;
        }
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Vec2};

    struct H {
        ctx: egui::Context,
        t: f64,
        last: DialogResult,
    }

    impl H {
        fn frame(&mut self, d: &mut ExportDialog, events: Vec<Event>) {
            self.t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 900.0))), time: Some(self.t), events, ..Default::default() };
            let mut res = DialogResult::Open;
            let mut out = self.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    // the content and the footer, as the dialog window shows them together
                    show(ui, d);
                    let r = footer(ui, d);
                    if r != DialogResult::Open {
                        res = r;
                    }
                });
            });
            out.textures_delta.clear();
            if res != DialogResult::Open {
                self.last = res;
            }
        }
        fn click(&mut self, d: &mut ExportDialog, key: &str) {
            let r = d.rects.iter().find(|(k, _)| k == key).map(|r| r.1).unwrap_or_else(|| panic!("no control {key}: {:?}", d.rects.iter().map(|r| &r.0).collect::<Vec<_>>()));
            self.click_at(d, r.center());
        }
        fn click_at(&mut self, d: &mut ExportDialog, p: Pos2) {
            let b = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            self.frame(d, vec![Event::PointerMoved(p)]);
            self.frame(d, vec![b(true)]);
            self.frame(d, vec![b(false)]);
            self.frame(d, vec![]);
        }
    }

    fn setup(drawing: bool) -> (H, ExportDialog) {
        let opts = if drawing { ExportOptions::for_drawing(&crate::drawing::Scene::parse(crate::drawing::SAMPLE_SCENE).unwrap()) } else { ExportOptions::for_note() };
        let mut d = ExportDialog::new(PathBuf::from("x.md"), drawing, "X".into(), opts, PathBuf::from("/tmp/x.pdf"));
        let mut h = H { ctx: egui::Context::default(), t: 0.0, last: DialogResult::Open };
        h.frame(&mut d, vec![]);
        h.frame(&mut d, vec![]);
        (h, d)
    }

    // spec: PDF-001
    #[test]
    fn note_dialog_controls() {
        let (mut h, mut d) = setup(false);
        for p in [Paper::Letter, Paper::A5, Paper::A3, Paper::Legal, Paper::A4] {
            h.click(&mut d, "paper");
            h.click(&mut d, &format!("paper:{}", p.label()));
            assert_eq!(d.opts.paper, p, "paper {p:?}");
        }
        h.click(&mut d, "landscape");
        assert!(d.opts.landscape);
        h.click(&mut d, "portrait");
        assert!(!d.opts.landscape);
        let m = d.rects.iter().find(|r| r.0 == "margin").unwrap().1;
        h.click_at(&mut d, Pos2::new(m.min.x + 3.0, m.center().y));
        assert!(d.opts.margin_mm < 5.0, "margin slider: {}", d.opts.margin_mm);
        let f = d.rects.iter().find(|r| r.0 == "font").unwrap().1;
        h.click_at(&mut d, Pos2::new(f.min.x + 3.0, f.center().y));
        assert!(d.opts.font_pt < 9.0, "text size slider: {}", d.opts.font_pt);
        assert!(!d.opts.plain_tables, "tables are a grid unless asked otherwise");
        for (key, get) in [("title", 0), ("pagenumbers", 1), ("embeds", 2), ("plain-tables", 3)] {
            let before = [d.opts.title, d.opts.page_numbers, d.opts.embeds, d.opts.plain_tables][get];
            h.click(&mut d, key);
            assert_ne!([d.opts.title, d.opts.page_numbers, d.opts.embeds, d.opts.plain_tables][get], before, "checkbox {key}");
        }
        h.click(&mut d, "open-after");
        assert!(d.open_after);
        h.click(&mut d, "export");
        assert_eq!(h.last, DialogResult::Export);
        h.click(&mut d, "cancel");
        assert_eq!(h.last, DialogResult::Cancel);
    }

    // spec: PDF-001
    #[test]
    fn drawing_dialog_controls() {
        let (mut h, mut d) = setup(true);
        assert_eq!(d.opts.paper, Paper::FitDrawing);
        for (key, b) in [("bg:Dark", Background::Dark), ("bg:Transparent", Background::None), ("bg:White", Background::White)] {
            h.click(&mut d, key);
            assert_eq!(d.opts.background, b, "{key}");
        }
        h.click(&mut d, "paper");
        h.click(&mut d, &format!("paper:{}", Paper::A4.label()));
        assert_eq!(d.opts.paper, Paper::A4);
        h.click(&mut d, "scale:actual");
        assert!(!d.opts.fill_page);
        h.click(&mut d, "scale:fill");
        assert!(d.opts.fill_page);
        h.click(&mut d, "portrait");
        assert!(!d.opts.landscape);
        h.click(&mut d, "landscape");
        assert!(d.opts.landscape);
        h.click(&mut d, "export");
        assert_eq!(h.last, DialogResult::Export);
    }
}
