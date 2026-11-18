//! The "Export to PDF" dialog: for a note or a folder the engine (built-in, or a LaTeX template:
//! TEX-016, BOOK-013), paper
//! size, orientation, margins, text size, what to include, background and scale for drawings,
//! where to save, and whether to open the PDF afterwards.

use crate::pdf::book_dialog::BookExport;
use crate::pdf::export::{Background, ExportOptions, Paper};
use crate::pdf::tex::dialog::TexExport;
use crate::theme;
use eframe::egui::{self, Rect, Ui};
use std::path::PathBuf;

/// What makes the PDF of a note: the built-in export or a template of a set through LaTeX (TEX-016).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ExportEngine {
    #[default]
    Builtin,
    Latex,
}

pub struct ExportDialog {
    pub rel: PathBuf,
    pub drawing: bool,
    pub name: String,
    pub opts: ExportOptions,
    pub path: String,
    pub open_after: bool,
    pub error: Option<String>,
    /// The engine of a note's or a folder's export (a drawing has the built-in one only).
    pub engine: ExportEngine,
    /// The LaTeX choices of a note or a folder; none for a drawing.
    pub tex: Option<Box<TexExport>>,
    /// A folder as one PDF (BOOK-010): its reading order and the job; none for a note or drawing.
    pub book: Option<Box<BookExport>>,
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
        ExportDialog { rel, drawing, name, opts, path: default_path.to_string_lossy().into_owned(), open_after: false, error: None, engine: ExportEngine::Builtin, tex: None, book: None, rects: vec![] }
    }

    /// A note's dialog with the LaTeX choices, on this engine.
    pub fn with_latex(mut self, tex: TexExport, engine: ExportEngine) -> Self {
        self.tex = Some(Box::new(tex));
        self.engine = engine;
        self
    }

    /// A folder's dialog: the book it makes.
    pub fn with_book(mut self, book: BookExport) -> Self {
        self.book = Some(Box::new(book));
        self
    }

    /// Whether this export goes through LaTeX.
    pub fn latex(&self) -> bool {
        self.engine == ExportEngine::Latex && self.tex.is_some()
    }
}

/// A size in millimetres without a needless `.0`.
fn trim_mm(v: f32) -> String {
    format!("{:.1}", v).trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn show(ui: &mut Ui, d: &mut ExportDialog) {
    d.rects.clear();
    let mut rects: Vec<(String, Rect)> = Vec::new();
    let mut rec = |k: &str, r: egui::Response| -> egui::Response {
        rects.push((k.to_string(), r.rect));
        r
    };
    let muted = |t: &str| egui::RichText::new(t).size(12.0).color(theme::MUTED);
    if d.tex.is_some() {
        let busy = d.tex.as_ref().is_some_and(|t| t.working()) || d.book.as_ref().is_some_and(|b| b.working());
        ui.horizontal(|ui| {
            ui.label(muted("Engine"));
            ui.add_enabled_ui(!busy, |ui| {
                for (e, label, key) in [(ExportEngine::Builtin, "Built-in", "engine:builtin"), (ExportEngine::Latex, "LaTeX template", "engine:latex")] {
                    if rec(key, ui.selectable_label(d.engine == e, label)).clicked() {
                        d.engine = e;
                    }
                }
            });
        });
        ui.add_space(4.0);
    }
    if let (ExportEngine::Latex, Some(tex)) = (d.engine, d.tex.as_deref_mut()) {
        tex.show(ui, &mut d.opts, &mut rec);
        ui.add_space(4.0);
        egui::Grid::new("export-save-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label(muted("Save as"));
            rec("path", ui.add(egui::TextEdit::singleline(&mut d.path).desired_width(250.0)));
            ui.end_row();
        });
        ui.add_space(4.0);
        // a folder's reading order is the same through LaTeX (BOOK-013)
        if let Some(book) = d.book.as_deref_mut() {
            book.show(ui, &mut rec);
            ui.add_space(4.0);
        }
        rec("open-after", ui.checkbox(&mut d.open_after, "Open the PDF after exporting"));
        d.rects = rects;
        return;
    }
    egui::Grid::new("export-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.label(muted("Paper"));
        let choices: &[Paper] = if d.drawing { &Paper::DRAWING } else { &Paper::NOTE };
        // a note can bring a paper size of its own (`print-paper: 215.9x279.4mm`, PDF-018)
        let custom = d.opts.custom_mm.map(|[w, h]| format!("Custom ({} × {} mm)", trim_mm(w), trim_mm(h)));
        let shown = custom.clone().unwrap_or_else(|| d.opts.paper.label().to_string());
        let combo = egui::ComboBox::from_id_salt("export-paper").width(250.0).selected_text(shown).show_ui(ui, |ui| {
            if let Some(label) = &custom {
                rec("paper:custom", ui.selectable_label(true, label));
            }
            for p in choices {
                if rec(&format!("paper:{}", p.label()), ui.selectable_label(custom.is_none() && d.opts.paper == *p, p.label())).clicked() {
                    d.opts.paper = *p;
                    d.opts.custom_mm = None;
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
        if d.drawing {
            let mut all = d.opts.margins_mm[0];
            if rec("margin", ui.add(egui::Slider::new(&mut all, 0.0..=40.0).suffix(" mm").step_by(1.0))).changed() {
                d.opts.margins_mm = [all; 4];
            }
        } else {
            // top, right, bottom, left (PDF-018)
            ui.horizontal(|ui| {
                for (side, (key, name)) in ["top", "right", "bottom", "left"].iter().zip(["↑", "→", "↓", "←"]).enumerate().map(|(i, (k, n))| (i, (*k, n))) {
                    ui.label(muted(name));
                    rec(&format!("margin:{key}"), ui.add(egui::DragValue::new(&mut d.opts.margins_mm[side]).range(0.0..=100.0).speed(0.5).suffix(" mm")).on_hover_text(key));
                }
            });
        }
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
            rec("font", ui.add(egui::Slider::new(&mut d.opts.font_pt, 4.0..=72.0).suffix(" pt").step_by(0.5)));
            ui.end_row();
            // what the note sets for itself and the export reads from it (PDF-013)
            if let Some(label) = d.opts.text_properties_label().filter(|_| d.book.is_none()) {
                ui.label(muted("From the note"));
                ui.label(muted(&label));
                ui.end_row();
            }
            ui.label(muted("Include"));
            ui.vertical(|ui| {
                rec("title", ui.checkbox(&mut d.opts.title, if d.book.is_some() { "Title page" } else { "The note's name as a title" }));
                rec("toc", ui.checkbox(&mut d.opts.toc, "Table of contents"));
                if d.book.is_some() {
                    rec("new-page-each", ui.checkbox(&mut d.opts.new_page_each, "Each note on a new page"));
                }
                rec("pagenumbers", ui.checkbox(&mut d.opts.page_numbers, "Page numbers"));
                rec("embeds", ui.checkbox(&mut d.opts.embeds, "Embedded pictures, drawings, PDFs, videos and sounds"));
                rec("plain-tables", ui.checkbox(&mut d.opts.plain_tables, "Export plain text (tables as raw text lines)"));
            });
            ui.end_row();
        }

        ui.label(muted("Save as"));
        rec("path", ui.add(egui::TextEdit::singleline(&mut d.path).desired_width(250.0)));
        ui.end_row();
    });
    ui.add_space(4.0);
    if let Some(book) = d.book.as_deref_mut() {
        book.show(ui, &mut rec);
        ui.add_space(4.0);
    }
    rec("open-after", ui.checkbox(&mut d.open_after, "Open the PDF after exporting"));
    d.rects = rects;
}

/// The dialog's footer (NAV-028): the error of the last try, Export and Cancel. Call it after
/// [`show`] in the same frame.
pub fn footer(ui: &mut Ui, d: &mut ExportDialog) -> DialogResult {
    let mut result = DialogResult::Open;
    if let Some(e) = &d.error {
        crate::theme::problem(ui, e);
    }
    ui.add_space(4.0);
    // LaTeX needs a TeX, a kind and no export running (TEX-016)
    let ready = (!d.latex() || d.tex.as_ref().is_some_and(|t| t.can_export())) && d.book.as_ref().is_none_or(|b| b.can_export());
    ui.horizontal(|ui| {
        let mut rec = |k: &str, r: egui::Response| -> egui::Response {
            d.rects.push((k.to_string(), r.rect));
            r
        };
        if rec("export", ui.add_enabled(ready, egui::Button::new(egui::RichText::new("Export").strong()))).clicked() {
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

    // spec: PDF-001, PDF-008
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
        for side in ["top", "right", "bottom", "left"] {
            assert!(d.rects.iter().any(|r| r.0 == format!("margin:{side}")), "a margin field for the {side}");
        }
        let f = d.rects.iter().find(|r| r.0 == "font").unwrap().1;
        h.click_at(&mut d, Pos2::new(f.min.x + 3.0, f.center().y));
        assert!(d.opts.font_pt < 9.0, "text size slider: {}", d.opts.font_pt);
        assert!(!d.opts.plain_tables, "tables are a grid unless asked otherwise");
        assert!(d.opts.toc, "a note gets a table of contents unless asked otherwise (PDF-008)");
        for (key, get) in [("title", 0), ("pagenumbers", 1), ("embeds", 2), ("plain-tables", 3), ("toc", 4)] {
            let before = [d.opts.title, d.opts.page_numbers, d.opts.embeds, d.opts.plain_tables, d.opts.toc][get];
            h.click(&mut d, key);
            assert_ne!([d.opts.title, d.opts.page_numbers, d.opts.embeds, d.opts.plain_tables, d.opts.toc][get], before, "checkbox {key}");
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
        assert!(!d.opts.toc, "a drawing has no contents (PDF-008)");
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
        let m = d.rects.iter().find(|r| r.0 == "margin").unwrap().1;
        h.click_at(&mut d, Pos2::new(m.min.x + 3.0, m.center().y));
        assert!(d.opts.margins_mm.iter().all(|m| *m < 5.0), "border slider: {:?}", d.opts.margins_mm);
        h.click(&mut d, "export");
        assert_eq!(h.last, DialogResult::Export);
    }

    // spec: PDF-018, PDF-019
    #[test]
    fn the_dialog_shows_the_notes_paper_margins_size_and_contents_and_what_the_note_sets_for_itself_pdf_019() {
        let text = "---\nprint-paper: 215.9x279.4mm\nprint-margins: 25.4 20 25.4 20\nprint-font-size: 12\nprint-toc: false\nprint-font: serif\nprint-align: justify\nprint-line-spacing: 1.15\n---\nx";
        let opts = ExportOptions::for_note().with_note_properties(text);
        let mut d = ExportDialog::new(PathBuf::from("x.md"), false, "X".into(), opts, PathBuf::from("/tmp/x.pdf"));
        let mut h = H { ctx: egui::Context::default(), t: 0.0, last: DialogResult::Open };
        h.frame(&mut d, vec![]);
        h.frame(&mut d, vec![]);
        assert!(!d.opts.toc && d.opts.font_pt == 12.0 && d.opts.margins_mm == [25.4, 20.0, 25.4, 20.0]);
        assert_eq!(d.opts.custom_mm, Some([215.9, 279.4]));
        // the custom paper is a choice of the list, and any other paper takes its place
        h.click(&mut d, "paper");
        assert!(d.rects.iter().any(|r| r.0 == "paper:custom"), "the custom size is listed");
        h.click(&mut d, &format!("paper:{}", Paper::A5.label()));
        assert_eq!((d.opts.paper, d.opts.custom_mm), (Paper::A5, None));
        assert_eq!(d.opts.text_properties_label().as_deref(), Some("serif, justified, line spacing 1.15"));
    }
}
