//! How big an embed is drawn (EDIT-024, MEDIA-012, PDF-005): one rule for the editor and the PDF
//! export, so a picture made smaller in the note, or the player of a sound, is as big in the PDF
//! in proportion to the text. The editor asks for a place `max_w` wide; the export asks for the
//! reference column (`REF_COLUMN`) and scales the result to its own text width (`to_page`).

use eframe::egui::Vec2;

/// The width of the text column the export assumes the editor had: sizes are worked out for it
/// and then scaled by the PDF's text width over this.
pub const REF_COLUMN: f32 = 720.0;
/// The tallest a picture or drawing is shown without a width in the link.
const PICTURE_MAX_H: f32 = 420.0;
/// The tallest the first page of an embedded PDF is shown.
const PDF_PAGE_MAX_H: f32 = 520.0;

/// `natural` scaled down (never up) to fit `max_w` × `max_h`.
pub fn fit(natural: Vec2, max_w: f32, max_h: f32) -> Vec2 {
    let k = (max_w / natural.x).min(max_h / natural.y).min(1.0);
    natural * k
}

/// A picture: the width its link asks for (up to the place), else fitted into the place and 420 points of height.
pub fn picture(natural: Vec2, asked: Option<f32>, max_w: f32) -> Vec2 {
    match asked {
        Some(a) => natural * (a.min(max_w) / natural.x),
        None => fit(natural, max_w, PICTURE_MAX_H),
    }
}

/// A drawing, from the size of its bounds (with the margin) in the scene.
pub fn drawing(bounds: Vec2, max_w: f32) -> Vec2 {
    fit(bounds.max(Vec2::splat(40.0)), max_w, PICTURE_MAX_H)
}

/// A page of an embedded PDF, from its size as rendered for the editor.
pub fn pdf_page(natural: Vec2, max_w: f32) -> Vec2 {
    fit(natural, max_w, PDF_PAGE_MAX_H)
}

/// The card of a file that is not downloaded yet (SYNC-020).
pub fn not_downloaded(max_w: f32) -> Vec2 {
    Vec2::new(max_w.min(520.0), 28.0)
}

/// An editor size as it is in the PDF: in proportion to the text width (`page_w`), but never
/// wider than it or taller than `max_h`.
pub fn to_page(size: Vec2, page_w: f32, max_h: f32) -> Vec2 {
    let scaled = size * (page_w / REF_COLUMN);
    scaled * (page_w / scaled.x).min(max_h / scaled.y).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: PDF-005
    #[test]
    fn sizes_follow_the_rule_of_the_editor() {
        let big = Vec2::new(1600.0, 800.0);
        assert_eq!(picture(big, None, 700.0), Vec2::new(700.0, 350.0), "fits the place");
        assert_eq!(picture(Vec2::new(100.0, 50.0), None, 700.0), Vec2::new(100.0, 50.0), "never bigger than itself");
        assert_eq!(picture(Vec2::new(500.0, 1000.0), None, 700.0), Vec2::new(210.0, 420.0), "and at most 420 high");
        assert_eq!(picture(big, Some(300.0), 700.0), Vec2::new(300.0, 150.0), "the width of the link");
        assert_eq!(picture(big, Some(900.0), 700.0).x, 700.0, "up to the place");
        assert_eq!(drawing(Vec2::new(10.0, 10.0), 700.0), Vec2::splat(40.0));
        assert_eq!(pdf_page(Vec2::new(450.0, 900.0), 700.0), Vec2::new(260.0, 520.0));
        assert_eq!(not_downloaded(900.0), Vec2::new(520.0, 28.0));
    }

    // spec: PDF-005
    #[test]
    fn the_page_gets_the_same_share_of_the_text_width() {
        // half of the column in the editor is half of the text in the PDF, on any page
        let half = Vec2::new(REF_COLUMN / 2.0, REF_COLUMN / 4.0);
        for page_w in [400.0, 483.0, 700.0] {
            let s = to_page(half, page_w, 10_000.0);
            assert!((s.x / page_w - 0.5).abs() < 1e-4 && (s.y / s.x - 0.5).abs() < 1e-4, "{page_w}: {s:?}");
        }
        assert_eq!(to_page(Vec2::new(REF_COLUMN * 2.0, REF_COLUMN), 400.0, 10_000.0), Vec2::new(400.0, 200.0), "never wider than the text");
        assert_eq!(to_page(Vec2::new(REF_COLUMN, REF_COLUMN), 400.0, 100.0), Vec2::new(100.0, 100.0), "nor taller than the page area");
    }
}
