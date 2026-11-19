//! Colours and egui style: a calm dark theme with a violet accent.

use crate::fonts::AreaFont;
use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

pub const BG_PRIMARY: Color32 = Color32::from_rgb(30, 30, 30);
pub const BG_SECONDARY: Color32 = Color32::from_rgb(38, 38, 38);
pub const BG_RIBBON: Color32 = Color32::from_rgb(33, 33, 33);
pub const BG_HOVER: Color32 = Color32::from_rgb(52, 52, 52);
pub const BG_ACTIVE: Color32 = Color32::from_rgb(60, 58, 74);
pub const BORDER: Color32 = Color32::from_rgb(54, 54, 54);
pub const TEXT: Color32 = Color32::from_rgb(218, 218, 218);
pub const TEXT_STRONG: Color32 = Color32::from_rgb(240, 240, 240);
pub const MUTED: Color32 = Color32::from_rgb(150, 150, 150);
pub const FAINT: Color32 = Color32::from_rgb(105, 105, 105);
pub const ACCENT: Color32 = Color32::from_rgb(138, 124, 247);
/// Links to notes that exist.
pub const LINK: Color32 = Color32::from_rgb(92, 166, 255);
/// Links to notes that do not exist yet.
pub const LINK_MISSING: Color32 = Color32::from_rgb(255, 104, 104);
pub const TAG: Color32 = Color32::from_rgb(170, 155, 255);
pub const TAG_BG: Color32 = Color32::from_rgb(52, 48, 78);
pub const CODE_BG: Color32 = Color32::from_rgb(42, 42, 46);
pub const CODE: Color32 = Color32::from_rgb(230, 170, 120);
/// The colour of a piece of highlighted code (EDIT-032); plain code keeps `CODE`. Shale has one
/// (dark) theme, so there is one palette, readable on `CODE_BG`.
pub fn syntax_colour(kind: crate::syntax::Kind) -> Color32 {
    use crate::syntax::Kind;
    match kind {
        Kind::Keyword => Color32::from_rgb(198, 142, 235),
        Kind::String => Color32::from_rgb(152, 205, 120),
        Kind::Number | Kind::Constant => Color32::from_rgb(240, 150, 100),
        Kind::Comment => Color32::from_rgb(120, 128, 140),
        Kind::Function => Color32::from_rgb(110, 175, 250),
        Kind::Type => Color32::from_rgb(230, 200, 110),
        Kind::Operator => Color32::from_rgb(110, 205, 210),
    }
}
pub const HEADING: Color32 = Color32::from_rgb(236, 236, 236);
pub const DONE: Color32 = Color32::from_rgb(120, 120, 120);
pub const QUOTE: Color32 = Color32::from_rgb(170, 170, 170);

/// Font family name of the bundled hand-drawn font (Virgil, SIL Open Font License).
pub const HAND_FONT: &str = "hand";

/// System fonts to hand to egui: the font data of every face by its key, and the families, each
/// a list of keys to look a character up in (CFG-033, CFG-039).
#[derive(Default)]
pub struct SystemFonts {
    pub data: Vec<(String, egui::FontData)>,
    pub families: Vec<(String, Vec<String>)>,
}

/// The fonts of egui: the normal ones, the bundled hand-drawn font as `FontFamily::Name("hand")`
/// and each family of `system` as a family of its own (all with the normal fonts as fallback for
/// characters they don't have).
pub fn font_definitions(system: SystemFonts) -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    let normal = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    defs.font_data.insert("virgil".into(), std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/Virgil.otf"))));
    let mut fam = vec!["virgil".to_string()];
    fam.extend(normal.clone());
    defs.families.insert(FontFamily::Name(HAND_FONT.into()), fam);
    for (key, data) in system.data {
        defs.font_data.insert(key, std::sync::Arc::new(data));
    }
    for (key, mut fam) in system.families {
        fam.extend(normal.clone());
        defs.families.insert(FontFamily::Name(key.into()), fam);
    }
    defs
}

/// Register the bundled hand-drawn font.
pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions(SystemFonts::default()));
}

/// The text styles for a font: the sizes of Small, Heading and Monospace follow the size of
/// the body text (14 points: 11.5, 20 and 13.5).
pub fn text_styles(font: &AreaFont) -> std::collections::BTreeMap<TextStyle, FontId> {
    let k = font.size / crate::fonts::UI_SIZE;
    [
        (TextStyle::Small, FontId::new(11.5 * k, font.family.clone())),
        (TextStyle::Body, FontId::new(14.0 * k, font.family.clone())),
        (TextStyle::Button, FontId::new(14.0 * k, font.family.clone())),
        (TextStyle::Heading, FontId::new(20.0 * k, font.family.clone())),
        (TextStyle::Monospace, FontId::new(13.5 * k, FontFamily::Monospace)),
    ]
    .into()
}

/// Use `font` for the text of the dialogs, menus and everything not in an area of its own.
pub fn set_text_styles(ctx: &egui::Context, font: &AreaFont) {
    ctx.global_style_mut(|s| s.text_styles = text_styles(font));
}

// ---- the spacing raster of dialogs and Settings (NAV-042, NAV-043)
//
// All on a 4-point grid, and each step clearly bigger than the one below it, so a reader sees
// what belongs together: the lines of one wrapped paragraph are closest, the items (rows,
// paragraphs, steps) of a page are further apart, and a new section is the furthest.
// They apply inside [`crate::dialog::Dialog`] only (through [`dialog_spacing`]); the main
// window keeps egui's tighter spacing.

/// The unit of the raster.
pub const GRID: f32 = 4.0;
/// Extra space between the wrapped lines of one paragraph or item, on top of the font's own
/// row height.
pub const LINE_GAP: f32 = GRID;
/// Space between two items one below the other: settings rows, paragraphs, steps.
pub const ITEM_GAP: f32 = 2.0 * GRID;
/// Space above a heading that starts a new section (the heading then sits closer to its own
/// items than to the section before it).
pub const SECTION_GAP: f32 = 6.0 * GRID;

/// The raster of dialogs for `ui` and everything inside it: the gap between items and the extra
/// line gap of plain text and of text edits.
pub fn dialog_spacing(ui: &mut egui::Ui) {
    let spacing = ui.spacing_mut();
    spacing.item_spacing.y = ITEM_GAP;
    spacing.extra_text_line_spacing = LINE_GAP;
}

/// `rich` with the line height of its font plus [`LINE_GAP`]. egui's `extra_text_line_spacing`
/// ([`dialog_spacing`] sets it) reaches the text of buttons, checkboxes and text edits, but not
/// a `Label`'s, so a paragraph that wraps gets its line gap through this (or [`prose`],
/// [`paragraph`], [`hint`]).
pub fn spaced(ui: &egui::Ui, rich: egui::RichText) -> egui::RichText {
    let row = ui.ctx().fonts_mut(|f| rich.font_height(f, ui.style()));
    rich.line_height(Some(row + LINE_GAP))
}

/// A paragraph of styled text with [`LINE_GAP`] between its lines.
pub fn prose(ui: &mut egui::Ui, rich: egui::RichText) -> egui::Response {
    let rich = spaced(ui, rich);
    ui.label(rich)
}

/// A paragraph of plain text in the normal text colour, with [`LINE_GAP`] between its lines.
pub fn paragraph(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    prose(ui, egui::RichText::new(text))
}

/// A paragraph in the colour of problems and errors, with [`LINE_GAP`] between its lines.
pub fn problem(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    prose(ui, egui::RichText::new(text).color(LINK_MISSING))
}

/// A grey 12-point hint or explanation, with [`LINE_GAP`] between its lines.
pub fn hint(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    prose(ui, egui::RichText::new(text).color(MUTED).size(12.0))
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_visuals(egui::Visuals::dark());
    let mut style = (*ctx.global_style()).clone();
    style.text_styles = text_styles(&AreaFont::shale(crate::fonts::UI_SIZE));
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.scroll.bar_width = 8.0;
    let v = &mut style.visuals;
    v.panel_fill = BG_SECONDARY;
    v.window_fill = BG_SECONDARY;
    v.extreme_bg_color = BG_PRIMARY;
    v.faint_bg_color = Color32::from_rgb(36, 36, 36);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.selection.bg_fill = Color32::from_rgb(70, 64, 130);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = LINK;
    v.override_text_color = None;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.bg_fill = Color32::from_rgb(48, 48, 48);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(44, 44, 44);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.corner_radius = CornerRadius::same(5);
    v.widgets.hovered.bg_fill = BG_HOVER;
    v.widgets.hovered.weak_bg_fill = BG_HOVER;
    v.widgets.hovered.corner_radius = CornerRadius::same(5);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(70, 70, 70));
    v.widgets.active.bg_fill = BG_ACTIVE;
    v.widgets.active.weak_bg_fill = BG_ACTIVE;
    v.widgets.active.corner_radius = CornerRadius::same(5);
    v.widgets.open.corner_radius = CornerRadius::same(5);
    v.text_cursor.stroke = Stroke::new(2.0, ACCENT);
    ctx.set_global_style(style);
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Context, FontId, Label, RawInput, RichText, Vec2};

    const LONG: &str = "A long hint that wraps over several lines in a narrow column, so that the space between its lines can be measured.";

    /// Run `f` in a ui of width `width` with the font at its defaults.
    fn in_ui(width: f32, f: impl FnOnce(&mut egui::Ui)) {
        let ctx = Context::default();
        let raw = RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0))), ..Default::default() };
        let mut f = Some(f);
        let mut out = ctx.run_ui(raw, |ui| {
            ui.set_max_width(width);
            (f.take().expect("one frame"))(ui);
        });
        out.textures_delta.clear();
    }

    // spec: NAV-042
    #[test]
    fn nav_042_the_raster_is_on_the_grid_and_each_step_is_bigger_than_the_one_below() {
        for gap in [LINE_GAP, ITEM_GAP, SECTION_GAP] {
            assert_eq!(gap % GRID, 0.0, "{gap} is on the {GRID}-point grid");
        }
        const { assert!(LINE_GAP < ITEM_GAP && ITEM_GAP < SECTION_GAP, "line gap < item gap < section gap") };
        const { assert!(ITEM_GAP >= 2.0 * LINE_GAP, "the gap between items is clearly bigger than the one inside an item") };
    }

    // spec: NAV-042
    #[test]
    fn nav_042_dialog_spacing_sets_the_item_gap_and_the_line_gap_on_one_ui_only() {
        in_ui(300.0, |ui| {
            let before = (ui.spacing().item_spacing.y, ui.spacing().extra_text_line_spacing);
            ui.scope(|ui| {
                dialog_spacing(ui);
                assert_eq!((ui.spacing().item_spacing.y, ui.spacing().extra_text_line_spacing), (ITEM_GAP, LINE_GAP));
            });
            assert_eq!((ui.spacing().item_spacing.y, ui.spacing().extra_text_line_spacing), before, "outside the scope nothing changed");
        });
    }

    /// The distance between the first two rows of `label` laid out in `ui`, and the row height
    /// of its font.
    fn pitch(ui: &mut egui::Ui, label: Label, font: &FontId) -> (f32, f32) {
        let (_, galley, _) = label.layout_in_ui(ui);
        assert!(galley.rows.len() >= 3, "the text wraps: {} rows", galley.rows.len());
        (galley.rows[1].pos.y - galley.rows[0].pos.y, ui.fonts_mut(|f| f.row_height(font)))
    }

    // spec: NAV-043
    #[test]
    fn nav_043_a_hint_has_the_line_gap_between_its_lines_and_rich_text_without_it_has_none() {
        in_ui(200.0, |ui| {
            let font = FontId::proportional(12.0);
            let rich = || RichText::new(LONG).color(MUTED).size(12.0);
            let (plain_pitch, row) = pitch(ui, Label::new(rich()), &font);
            assert!((plain_pitch - row).abs() < 0.6, "rich text keeps the font's row height: {plain_pitch} vs {row}");
            let (spaced_pitch, _) = pitch(ui, Label::new(spaced(ui, rich())), &font);
            assert!((spaced_pitch - (row + LINE_GAP)).abs() < 0.6, "row + LINE_GAP: {spaced_pitch} vs {row} + {LINE_GAP}");
        });
    }

    // spec: NAV-043
    #[test]
    fn nav_043_hint_and_prose_draw_the_text_taller_by_the_line_gap_of_each_row() {
        in_ui(200.0, |ui| {
            let plain = ui.label(RichText::new(LONG).color(MUTED).size(12.0)).rect.height();
            let hint = hint(ui, LONG).rect.height();
            let row = ui.fonts_mut(|f| f.row_height(&FontId::proportional(12.0)));
            let rows = (plain / row).round();
            assert!(rows >= 3.0);
            assert!((hint - plain - rows * LINE_GAP).abs() < 1.5, "{rows} rows: plain {plain}, hint {hint}");
            let big = prose(ui, RichText::new(LONG).size(18.0)).rect.height();
            let plain_big = ui.label(RichText::new(LONG).size(18.0)).rect.height();
            assert!(big > plain_big + 2.0 * LINE_GAP, "prose of another size too: {big} vs {plain_big}");
            let plain_text = ui.label(LONG).rect.height();
            let paragraph = paragraph(ui, LONG).rect.height();
            assert!(paragraph > plain_text + 2.0 * LINE_GAP, "a paragraph of plain text too: {paragraph} vs {plain_text}");
        });
    }
}
