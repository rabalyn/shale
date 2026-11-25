//! The "Export to PDF" dialog: for a note or a folder the engine (built-in, or a LaTeX template:
//! TEX-016, BOOK-013), paper
//! size, orientation, margins, text size, what to include, background and scale for drawings,
//! where to save, and whether to open the PDF afterwards.

use crate::busy::Busy;
use crate::pdf::book_dialog::{BookExport, PDF_EXPORT};
use crate::pdf::export::{Background, ExportOptions, Paper};
use crate::pdf::note_export::NoteExport;
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
    /// The export of a note or drawing with the built-in writer, while it runs (JOB-017).
    pub note: Option<NoteExport>,
    /// The fonts the export will use for this note and what could not be used (PDF-034, PDF-037);
    /// filled by the window once the font scan is in (JOB-040).
    pub fonts: Option<FontsNote>,
    /// Since when the export waits for the font scan to finish (PDF-037, ADR-0040 §8); none when it does not.
    pub fonts_wait: Option<std::time::Instant>,
    busy: Busy,
    /// screen rect of every control (used by the UI tests)
    pub rects: Vec<(String, Rect)>,
}

/// The fonts of a note's export in words, for the dialog (PDF-019).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontsNote {
    /// What the scan and the engine of the dialog were when this was made; it is made again when they change.
    pub stamp: (u64, bool, usize),
    /// "text Garamond (system), code Hack": the roles that the note, the book or the vault set.
    pub line: String,
    pub warnings: Vec<String>,
    /// The system fonts are still being looked up.
    pub pending: bool,
}

impl FontsNote {
    /// The row(s) of the dialog: the fonts that are set, and why one was not used.
    fn show(&self, ui: &mut Ui, muted: &dyn Fn(&str) -> egui::RichText) {
        if self.line.is_empty() && self.warnings.is_empty() && !self.pending {
            return;
        }
        ui.label(muted("Fonts"));
        ui.vertical(|ui| {
            if !self.line.is_empty() {
                ui.label(muted(&self.line));
            }
            if self.pending {
                ui.label(muted("Looking up fonts..."));
            }
            for w in &self.warnings {
                crate::theme::problem(ui, w);
            }
        });
        ui.end_row();
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum DialogResult {
    Open,
    Export,
    Cancel,
}

impl ExportDialog {
    pub fn new(rel: PathBuf, drawing: bool, name: String, opts: ExportOptions, default_path: PathBuf) -> Self {
        ExportDialog {
            rel,
            drawing,
            name,
            opts,
            path: default_path.to_string_lossy().into_owned(),
            open_after: false,
            error: None,
            engine: ExportEngine::Builtin,
            tex: None,
            book: None,
            note: None,
            fonts: None,
            fonts_wait: None,
            busy: Busy::default(),
            rects: vec![],
        }
    }

    /// The registry the exports of this dialog are tasks of (the window's; a dialog made without
    /// one has its own).
    pub fn with_busy(mut self, busy: &Busy) -> Self {
        self.busy = busy.clone();
        self
    }

    /// The registry the exports of this dialog are tasks of.
    pub fn busy(&self) -> &Busy {
        &self.busy
    }

    /// Whether the built-in export of a note or drawing runs.
    pub fn note_working(&self) -> bool {
        self.note.is_some()
    }

    /// Stop the export of a note or drawing that is running.
    pub fn cancel_note(&mut self) {
        self.note = None;
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

/// The box "Start each # heading on a new page" (PDF-021): on when the note or the dialog has a
/// level, which it then shows (`#` for 1, `##` for 2, …) as a small field to change; ticking it
/// on takes level 1. `enabled` false: shown off and greyed (a kind of document without them).
pub(crate) fn break_before_box(ui: &mut Ui, opts: &mut ExportOptions, enabled: bool, rec: &mut dyn FnMut(&str, egui::Response) -> egui::Response) {
    ui.horizontal(|ui| {
        ui.add_enabled_ui(enabled, |ui| {
            let mut on = enabled && opts.break_before > 0;
            if rec("break-before", ui.checkbox(&mut on, "Start each # heading on a new page")).on_disabled_hover_text("This kind of document has no pages for headings to start").changed() {
                opts.break_before = u8::from(on);
            }
            if on {
                let level = egui::DragValue::new(&mut opts.break_before).range(1..=6).speed(0.05).custom_formatter(|n, _| format!("to {}", "#".repeat(n as usize)));
                rec("break-before-level", ui.add(level)).on_hover_text("The deepest heading level that starts a page: # is 1, ## is 2, …");
            }
        });
    });
}

/// The box "Page break after the table of contents" (PDF-031), on by default: greyed while there is no
/// table of contents (`enabled` false: the box is off or the kind of document has none).
pub(crate) fn toc_break_box(ui: &mut Ui, opts: &mut ExportOptions, enabled: bool, rec: &mut dyn FnMut(&str, egui::Response) -> egui::Response) {
    ui.add_enabled_ui(enabled, |ui| {
        rec("toc-break", ui.checkbox(&mut opts.toc_break, "Page break after the table of contents")).on_disabled_hover_text("There is no table of contents");
    });
}

/// The boxes of the thesis syntax (PDF-032): the lists of figures and of tables after the contents
/// (a marker line in the note puts a list where it stands instead), and the citation style, which
/// takes the place of the note's `citation-style:`.
pub(crate) fn thesis_boxes(ui: &mut Ui, opts: &mut ExportOptions, rec: &mut dyn FnMut(&str, egui::Response) -> egui::Response) {
    rec("list-of-figures", ui.checkbox(&mut opts.list_of_figures, "List of figures")).on_hover_text("After the contents; a line <!-- shale: list-of-figures --> in the note puts it there instead");
    rec("list-of-tables", ui.checkbox(&mut opts.list_of_tables, "List of tables")).on_hover_text("After the contents; a line <!-- shale: list-of-tables --> in the note puts it there instead");
    ui.horizontal(|ui| {
        ui.label("Citation style");
        let shown = opts.citation_style.map_or("From the note", |s| s.name());
        let combo = egui::ComboBox::from_id_salt("citation-style").selected_text(shown).show_ui(ui, |ui| {
            rec("citation-style:note", ui.selectable_value(&mut opts.citation_style, None, "From the note"));
            for style in crate::bibliography::format::Style::ALL {
                rec(&format!("citation-style:{}", style.name()), ui.selectable_value(&mut opts.citation_style, Some(style), style.name()));
            }
        });
        rec("citation-style", combo.response);
    });
}

pub fn show(ui: &mut Ui, d: &mut ExportDialog) {
    d.rects.clear();
    let mut rects: Vec<(String, Rect)> = Vec::new();
    let mut rec = |k: &str, r: egui::Response| -> egui::Response {
        rects.push((k.to_string(), r.rect));
        r
    };
    let muted = |t: &str| egui::RichText::new(t).size(12.0).color(theme::palette().muted);
    if d.tex.is_some() {
        let busy = d.tex.as_ref().is_some_and(|t| t.working()) || d.book.as_ref().is_some_and(|b| b.working()) || d.note_working();
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
    }
    if let (ExportEngine::Latex, Some(tex)) = (d.engine, d.tex.as_deref_mut()) {
        tex.show(ui, &mut d.opts, &mut rec);
        egui::Grid::new("export-save-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            if let Some(f) = d.fonts.as_ref().filter(|_| d.book.is_none()) {
                f.show(ui, &muted);
            }
            ui.label(muted("Save as"));
            rec("path", ui.add(egui::TextEdit::singleline(&mut d.path).desired_width(250.0)));
            ui.end_row();
        });
        // a folder's reading order is the same through LaTeX (BOOK-013)
        if let Some(book) = d.book.as_deref_mut() {
            book.show(ui, &mut rec);
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
            if let Some(f) = d.fonts.as_ref().filter(|_| d.book.is_none()) {
                f.show(ui, &muted);
            }
            ui.label(muted("Include"));
            ui.vertical(|ui| {
                rec("title", ui.checkbox(&mut d.opts.title, if d.book.is_some() { "Title page" } else { "The note's name as a title" }));
                rec("toc", ui.checkbox(&mut d.opts.toc, "Table of contents"));
                let on = d.opts.toc;
                toc_break_box(ui, &mut d.opts, on, &mut rec);
                if d.book.is_some() {
                    rec("new-page-each", ui.checkbox(&mut d.opts.new_page_each, "Each note on a new page"));
                }
                if d.book.is_none() {
                    break_before_box(ui, &mut d.opts, true, &mut rec);
                    thesis_boxes(ui, &mut d.opts, &mut rec);
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
    if let Some(book) = d.book.as_deref_mut() {
        book.show(ui, &mut rec);
    }
    if d.fonts_wait.is_some() {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(14.0));
            rec("waiting-for-fonts", ui.label(muted("Looking up fonts… the export starts then. Cancel stops it.")));
        });
    }
    if d.note_working() {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(14.0));
            rec("working", ui.label(muted("Exporting to PDF… Cancel stops it.")));
        });
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
    // LaTeX needs a TeX, a kind and no export running (TEX-016)
    // the built-in export of a note: none runs, here or in a dialog that was closed while it stopped (JOB-005)
    let note_free = d.latex() || d.book.is_some() || (d.note.is_none() && !d.busy.is_running(&PDF_EXPORT));
    let ready = (!d.latex() || d.tex.as_ref().is_some_and(|t| t.can_export(&d.busy))) && d.book.as_ref().is_none_or(|b| b.can_export()) && note_free && d.fonts_wait.is_none();
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

    // spec: PDF-001, PDF-008, EDIT-024, PDF-019
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

    // spec: PDF-018, PDF-019, PDF-024
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
        assert_eq!(d.opts.text_properties_label().as_deref(), Some("paper look, serif, justified, line spacing 1.15"));
    }

    // spec: PDF-021
    #[test]
    fn the_dialog_has_a_box_for_starting_headings_on_new_pages_filled_from_the_note_pdf_021() {
        let has = |d: &ExportDialog, key: &str| d.rects.iter().any(|r| r.0 == key);
        // a note without the property: off, no level shown; ticking it takes level 1
        let (mut h, mut d) = setup(false);
        assert_eq!(d.opts.break_before, 0);
        assert!(has(&d, "break-before") && !has(&d, "break-before-level"));
        h.click(&mut d, "break-before");
        assert_eq!(d.opts.break_before, 1);
        assert!(has(&d, "break-before-level"), "the level can be changed once it is on");
        h.click(&mut d, "break-before");
        assert_eq!(d.opts.break_before, 0);
        // a note that says 2: the box is on with the level it says
        let opts = ExportOptions::for_note().with_note_properties("---\nprint-break-before: 2\n---\nx");
        let mut d = ExportDialog::new(PathBuf::from("x.md"), false, "X".into(), opts, PathBuf::from("/tmp/x.pdf"));
        let mut h = H { ctx: egui::Context::default(), t: 0.0, last: DialogResult::Open };
        h.frame(&mut d, vec![]);
        h.frame(&mut d, vec![]);
        assert_eq!(d.opts.break_before, 2);
        assert!(has(&d, "break-before-level"));
        h.click(&mut d, "break-before");
        assert_eq!(d.opts.break_before, 0, "unticked: none");
        // a drawing has no headings, a folder as a book has its own page rules (BOOK-005)
        let (_, d) = setup(true);
        assert!(!has(&d, "break-before"));
    }

    // spec: PDF-032
    #[test]
    fn pdf_032_the_dialog_has_boxes_for_the_lists_of_figures_and_tables_and_a_citation_style() {
        let has = |d: &ExportDialog, key: &str| d.rects.iter().any(|r| r.0 == key);
        let (mut h, mut d) = setup(false);
        assert!(has(&d, "list-of-figures") && has(&d, "list-of-tables") && has(&d, "citation-style"));
        assert!(!d.opts.list_of_figures && !d.opts.list_of_tables && d.opts.citation_style.is_none(), "off, and the note's style, by default");
        h.click(&mut d, "list-of-figures");
        h.click(&mut d, "list-of-tables");
        assert!(d.opts.list_of_figures && d.opts.list_of_tables);
        h.click(&mut d, "list-of-figures");
        assert!(!d.opts.list_of_figures);
        h.click(&mut d, "citation-style");
        h.click(&mut d, "citation-style:numeric");
        assert_eq!(d.opts.citation_style, Some(crate::bibliography::format::Style::Numeric));
        // a drawing has no thesis, and a folder as a book has them in a later phase
        let (_, d) = setup(true);
        assert!(!has(&d, "list-of-figures") && !has(&d, "citation-style"));
    }

    // spec: PDF-031
    #[test]
    fn the_dialog_has_a_box_for_the_page_break_after_the_table_of_contents_filled_from_the_note_pdf_031() {
        let (mut h, mut d) = setup(false);
        assert!(d.rects.iter().any(|r| r.0 == "toc-break") && d.opts.toc_break, "there, and on by default");
        h.click(&mut d, "toc-break");
        assert!(!d.opts.toc_break);
        h.click(&mut d, "toc-break");
        assert!(d.opts.toc_break);
        // the box belongs to the contents: with those off it is greyed and a click changes nothing
        h.click(&mut d, "toc");
        h.click(&mut d, "toc-break");
        assert!(!d.opts.toc && d.opts.toc_break);
        // a note that says no: the dialog opens with the box off
        let opts = ExportOptions::for_note().with_note_properties("---\nprint-toc-page-break: false\n---\nx");
        let mut d = ExportDialog::new(PathBuf::from("x.md"), false, "X".into(), opts, PathBuf::from("/tmp/x.pdf"));
        let mut h = H { ctx: egui::Context::default(), t: 0.0, last: DialogResult::Open };
        h.frame(&mut d, vec![]);
        h.frame(&mut d, vec![]);
        assert!(!d.opts.toc_break);
        // a drawing has no contents and so no such box
        let (_, d) = setup(true);
        assert!(!d.rects.iter().any(|r| r.0 == "toc-break"));
    }

    // spec: JOB-005, JOB-010, JOB-017
    #[test]
    fn export_is_off_while_a_built_in_export_runs_in_this_dialog_or_another_and_the_dialog_says_it_works() {
        use crate::pdf::note_export::{NoteExport, What, Work};
        let busy = Busy::default();
        let opts = ExportOptions::for_note();
        let mut d = ExportDialog::new(PathBuf::from("x.md"), false, "X".into(), opts.clone(), PathBuf::from("/tmp/x.pdf")).with_busy(&busy);
        let mut h = H { ctx: egui::Context::default(), t: 0.0, last: DialogResult::Open };
        h.frame(&mut d, vec![]);
        h.frame(&mut d, vec![]);
        assert!(!d.note_working() && !d.rects.iter().any(|r| r.0 == "working"));
        // an export of a folder (or a dialog that was closed while its export stopped) holds the key
        let held = busy.begin(PDF_EXPORT, "Exporting “Handbook” to PDF…").unwrap();
        h.click(&mut d, "export");
        assert_eq!(h.last, DialogResult::Open, "Export is off beside another export (JOB-005)");
        drop(held);
        h.click(&mut d, "export");
        assert_eq!(h.last, DialogResult::Export);
        // this dialog's own export: the spinner and the line, Export off, Cancel still there
        let root = std::env::temp_dir().join(format!("shale-dialog-note-job-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        d.note = NoteExport::start(&busy, Work { root: root.clone(), rel: PathBuf::from("x.md"), name: "X".into(), opts, what: What::Note("x\n".into()) }, || {});
        assert!(d.note_working());
        h.last = DialogResult::Open;
        h.frame(&mut d, vec![]);
        assert!(d.rects.iter().any(|r| r.0 == "working"), "the dialog says that it works");
        h.click(&mut d, "export");
        assert_eq!(h.last, DialogResult::Open, "no second export beside the running one");
        h.click(&mut d, "cancel");
        assert_eq!(h.last, DialogResult::Cancel, "Cancel is there while it works");
        d.cancel_note();
        assert!(!d.note_working());
        let _ = std::fs::remove_dir_all(&root);
    }
}
