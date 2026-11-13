//! Colours and egui style: a calm dark theme with a violet accent.

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
pub const HEADING: Color32 = Color32::from_rgb(236, 236, 236);
pub const DONE: Color32 = Color32::from_rgb(120, 120, 120);
pub const QUOTE: Color32 = Color32::from_rgb(170, 170, 170);

/// Font family name of the bundled hand-drawn font (Virgil, SIL Open Font License).
pub const HAND_FONT: &str = "hand";

/// Register the bundled hand-drawn font as `FontFamily::Name("hand")` (with the normal fonts as
/// fallback for characters it doesn't have).
pub fn install_fonts(ctx: &egui::Context) {
    let mut defs = egui::FontDefinitions::default();
    defs.font_data.insert("virgil".into(), std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/Virgil.otf"))));
    let mut fam = vec!["virgil".to_string()];
    fam.extend(defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    defs.families.insert(FontFamily::Name(HAND_FONT.into()), fam);
    ctx.set_fonts(defs);
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_visuals(egui::Visuals::dark());
    let mut style = (*ctx.global_style()).clone();
    style.text_styles = [
        (TextStyle::Small, FontId::new(11.5, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(20.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(13.5, FontFamily::Monospace)),
    ]
    .into();
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
