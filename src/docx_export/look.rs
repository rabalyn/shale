//! What a note's `print-*` properties say, each one only when the note has it and its value is
//! good (DOCX-008, DOCX-009). The parsing is the PDF export's (`pdf::export`, PDF-013, PDF-023):
//! the same values are good or bad in both.

use crate::pdf::export::{self, NotePaper, Paper};
use crate::pdf::write::{FontKind, Rgba};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrintLook {
    /// the page in millimetres, width and height as the note says them (wider than tall: landscape)
    pub paper: Option<[f32; 2]>,
    /// top, right, bottom, left in millimetres
    pub margins: Option<[f32; 4]>,
    pub font: Option<FontKind>,
    pub font_pt: Option<f32>,
    pub line_spacing: Option<f32>,
    pub paragraph_spacing: Option<f32>,
    /// `print-align: justify`
    pub justify: bool,
    pub heading_color: Option<Rgba>,
    pub heading_bold: Option<bool>,
    pub heading_sizes: Option<[f32; 6]>,
    /// the deepest heading level that starts a page, 1 to 6
    pub break_before: Option<u8>,
    /// `print-toc-page-break`: whether what follows the table of contents starts a page (DOCX-013); the export
    /// replaces it with the choice of its dialog
    pub toc_page_break: Option<bool>,
}

/// The size of a standard paper in millimetres, portrait.
fn standard_mm(p: Paper) -> Option<[f32; 2]> {
    Some(match p {
        Paper::A3 => [297.0, 420.0],
        Paper::A4 => [210.0, 297.0],
        Paper::A5 => [148.0, 210.0],
        Paper::Letter => [215.9, 279.4],
        Paper::Legal => [215.9, 355.6],
        Paper::FitDrawing => return None,
    })
}

impl PrintLook {
    /// Whether the note says anything that changes the styles of the reference document (DOCX-008); the page is the file's.
    pub fn styled(&self) -> bool {
        self.font.is_some()
            || self.font_pt.is_some()
            || self.line_spacing.is_some()
            || self.paragraph_spacing.is_some()
            || self.justify
            || self.heading_color.is_some()
            || self.heading_bold.is_some()
            || self.heading_sizes.is_some()
            || self.break_before.is_some()
    }

    /// The properties of the front matter of `text`.
    pub fn of(text: &str) -> PrintLook {
        let props = export::print_properties(text);
        let get = |k: &str| props.get(k).map(String::as_str);
        let paper = get("print-paper").and_then(export::parse_paper).and_then(|p| match p {
            NotePaper::Standard(p) => standard_mm(p),
            NotePaper::Custom(mm) => Some(mm),
        });
        PrintLook {
            paper,
            margins: get("print-margins").and_then(export::parse_margins),
            font: get("print-font").and_then(|v| match v.to_lowercase().as_str() {
                "serif" => Some(FontKind::Serif),
                "sans" => Some(FontKind::Sans),
                "mono" => Some(FontKind::Mono),
                _ => None,
            }),
            font_pt: get("print-font-size").and_then(export::number).filter(|v| (4.0..=72.0).contains(v)),
            line_spacing: get("print-line-spacing").and_then(export::number).filter(|v| (0.8..=3.0).contains(v)),
            paragraph_spacing: get("print-paragraph-spacing").and_then(export::number).filter(|v| (0.0..=72.0).contains(v)),
            justify: get("print-align").is_some_and(|v| v.eq_ignore_ascii_case("justify")),
            heading_color: get("print-heading-color").and_then(export::parse_hex_colour),
            heading_bold: match get("print-heading-weight").map(str::to_lowercase).as_deref() {
                Some("bold") => Some(true),
                Some("normal") => Some(false),
                _ => None,
            },
            heading_sizes: get("print-heading-sizes").and_then(export::parse_heading_sizes),
            break_before: get("print-break-before").and_then(|v| v.trim().parse::<u8>().ok()).filter(|l| (1..=6).contains(l)),
            toc_page_break: get("print-toc-page-break").and_then(export::parse_bool),
        }
    }
}
