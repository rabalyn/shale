//! Colours and egui style: the palette of the theme (a calm dark theme with a violet accent by
//! default) and the spacing raster.

use crate::fonts::AreaFont;
use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

/// Every colour of the window (CFG-061). A theme is one palette; [`Palette::shale`] is the
/// default and has the colours the app always had.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// Whether egui's dark visuals are the base (`false`: its light ones).
    pub dark: bool,
    pub bg_primary: Color32,
    pub bg_secondary: Color32,
    pub bg_ribbon: Color32,
    pub bg_hover: Color32,
    pub bg_active: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_strong: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    /// Links to notes that exist.
    pub link: Color32,
    /// Links to notes that do not exist yet (and problems).
    pub link_missing: Color32,
    pub tag: Color32,
    pub tag_bg: Color32,
    pub code_bg: Color32,
    pub code: Color32,
    pub heading: Color32,
    pub done: Color32,
    pub quote: Color32,
    pub syntax_keyword: Color32,
    pub syntax_string: Color32,
    pub syntax_number: Color32,
    pub syntax_comment: Color32,
    pub syntax_function: Color32,
    pub syntax_type: Color32,
    pub syntax_operator: Color32,
    /// Fill of every other row and of faint areas.
    pub faint_bg: Color32,
    /// Background of selected text.
    pub selection_bg: Color32,
    /// Fill of a button or a field at rest.
    pub widget_bg: Color32,
    /// Fill of a flat button at rest.
    pub widget_weak_bg: Color32,
    /// Outline of a hovered widget.
    pub hover_stroke: Color32,
    /// Background of `==highlighted==` text (the default highlight, not a `<mark>` colour).
    pub highlight_bg: Color32,
    /// Fill of a banner that reports a problem.
    pub banner_error_bg: Color32,
    /// Fill of a banner that gives a notice.
    pub banner_notice_bg: Color32,
    /// Text of something that is fine in Settings and set-up dialogs.
    pub ok_text: Color32,
    /// Foreground and background of a good, a warning and a bad status (the mail security
    /// badges); the foreground of a bad one is `link_missing`.
    pub good: Color32,
    pub good_bg: Color32,
    pub warn: Color32,
    pub warn_bg: Color32,
    pub bad_bg: Color32,
    /// The lines between the notes of the graph, with a note highlighted, and without.
    pub graph_edge_dim: Color32,
    pub graph_edge: Color32,
    /// The notes of the graph: a note, a drawing.
    pub graph_node: Color32,
    pub graph_node_drawing: Color32,
}

impl Palette {
    /// The theme Shale: a calm dark theme with a violet accent.
    pub const fn shale() -> Self {
        Self {
            dark: true,
            bg_primary: Color32::from_rgb(30, 30, 30),
            bg_secondary: Color32::from_rgb(38, 38, 38),
            bg_ribbon: Color32::from_rgb(33, 33, 33),
            bg_hover: Color32::from_rgb(52, 52, 52),
            bg_active: Color32::from_rgb(60, 58, 74),
            border: Color32::from_rgb(54, 54, 54),
            text: Color32::from_rgb(218, 218, 218),
            text_strong: Color32::from_rgb(240, 240, 240),
            muted: Color32::from_rgb(150, 150, 150),
            faint: Color32::from_rgb(105, 105, 105),
            accent: Color32::from_rgb(138, 124, 247),
            link: Color32::from_rgb(92, 166, 255),
            link_missing: Color32::from_rgb(255, 104, 104),
            tag: Color32::from_rgb(170, 155, 255),
            tag_bg: Color32::from_rgb(52, 48, 78),
            code_bg: Color32::from_rgb(42, 42, 46),
            code: Color32::from_rgb(230, 170, 120),
            heading: Color32::from_rgb(236, 236, 236),
            done: Color32::from_rgb(120, 120, 120),
            quote: Color32::from_rgb(170, 170, 170),
            syntax_keyword: Color32::from_rgb(198, 142, 235),
            syntax_string: Color32::from_rgb(152, 205, 120),
            syntax_number: Color32::from_rgb(240, 150, 100),
            syntax_comment: Color32::from_rgb(120, 128, 140),
            syntax_function: Color32::from_rgb(110, 175, 250),
            syntax_type: Color32::from_rgb(230, 200, 110),
            syntax_operator: Color32::from_rgb(110, 205, 210),
            faint_bg: Color32::from_rgb(36, 36, 36),
            selection_bg: Color32::from_rgb(70, 64, 130),
            widget_bg: Color32::from_rgb(48, 48, 48),
            widget_weak_bg: Color32::from_rgb(44, 44, 44),
            hover_stroke: Color32::from_rgb(70, 70, 70),
            highlight_bg: Color32::from_rgba_premultiplied(47, 40, 9, 60),
            banner_error_bg: Color32::from_rgb(64, 44, 44),
            banner_notice_bg: Color32::from_rgb(48, 46, 60),
            ok_text: Color32::from_rgb(110, 200, 130),
            good: Color32::from_rgb(110, 205, 130),
            good_bg: Color32::from_rgb(30, 52, 38),
            warn: Color32::from_rgb(235, 185, 85),
            warn_bg: Color32::from_rgb(58, 48, 28),
            bad_bg: Color32::from_rgb(66, 32, 32),
            graph_edge_dim: Color32::from_gray(50),
            graph_edge: Color32::from_gray(78),
            graph_node: Color32::from_gray(170),
            graph_node_drawing: Color32::from_rgb(120, 190, 140),
        }
    }

    /// The colour of a piece of highlighted code (EDIT-032); plain code keeps `code`.
    pub fn syntax(&self, kind: crate::syntax::Kind) -> Color32 {
        use crate::syntax::Kind;
        match kind {
            Kind::Keyword => self.syntax_keyword,
            Kind::String => self.syntax_string,
            Kind::Number | Kind::Constant => self.syntax_number,
            Kind::Comment => self.syntax_comment,
            Kind::Function => self.syntax_function,
            Kind::Type => self.syntax_type,
            Kind::Operator => self.syntax_operator,
        }
    }
}

/// Names every colour field of [`Palette`] once: the keys of the `[shale]` table of a theme file
/// (CFG-063) and the way to reach a field by its name.
macro_rules! palette_slots {
    ($($field:ident),* $(,)?) => {
        impl Palette {
            /// The names of the colour slots (every field but `dark`).
            pub const SLOTS: &'static [&'static str] = &[$(stringify!($field)),*];

            /// The colour slot with this name.
            pub fn slot_mut(&mut self, name: &str) -> Option<&mut Color32> {
                match name {
                    $(stringify!($field) => Some(&mut self.$field),)*
                    _ => None,
                }
            }
        }
    };
}

palette_slots!(
    bg_primary,
    bg_secondary,
    bg_ribbon,
    bg_hover,
    bg_active,
    border,
    text,
    text_strong,
    muted,
    faint,
    accent,
    link,
    link_missing,
    tag,
    tag_bg,
    code_bg,
    code,
    heading,
    done,
    quote,
    syntax_keyword,
    syntax_string,
    syntax_number,
    syntax_comment,
    syntax_function,
    syntax_type,
    syntax_operator,
    faint_bg,
    selection_bg,
    widget_bg,
    widget_weak_bg,
    hover_stroke,
    highlight_bg,
    banner_error_bg,
    banner_notice_bg,
    ok_text,
    good,
    good_bg,
    warn,
    warn_bg,
    bad_bg,
    graph_edge_dim,
    graph_edge,
    graph_node,
    graph_node_drawing
);

thread_local! {
    /// The palette of this thread, which is the UI thread: the one that draws. Another thread
    /// (a job, a worker) starts with Shale and must be handed a palette if it needs one.
    static PALETTE: std::cell::Cell<Palette> = const { std::cell::Cell::new(Palette::shale()) };
    /// How many times the palette of this thread was changed ([`generation`]).
    static GENERATION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The palette of the current theme on this (the UI) thread (CFG-061).
pub fn palette() -> Palette {
    PALETTE.with(std::cell::Cell::get)
}

/// Make `palette` the one of this (the UI) thread; [`apply`] or [`apply_palette`] puts it into
/// egui's visuals (CFG-061).
pub fn set_palette(palette: Palette) {
    if PALETTE.with(|p| p.replace(palette)) != palette {
        GENERATION.with(|g| g.set(g.get() + 1));
    }
}

/// A number that changes whenever the palette of this (the UI) thread does (CFG-062). What bakes
/// the palette's colours into something it keeps (a laid-out galley, formatted blocks) keeps the
/// generation with it and builds again when it differs, like [`crate::syntax::generation`].
pub fn generation() -> u64 {
    GENERATION.with(std::cell::Cell::get)
}

/// Font family name of the bundled hand-drawn font (Virgil, SIL Open Font License).
pub const HAND_FONT: &str = "hand";

/// System fonts to hand to egui: the font data of every face by its key, and the families, each
/// a list of keys to look a character up in (CFG-033, CFG-039).
#[derive(Default)]
pub struct SystemFonts {
    pub data: Vec<(String, std::sync::Arc<egui::FontData>)>,
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
        defs.font_data.insert(key, data);
    }
    for (key, mut fam) in system.families {
        fam.extend(normal.clone());
        defs.families.insert(FontFamily::Name(key.into()), fam);
    }
    defs
}

/// Register the bundled hand-drawn font.
pub fn install_fonts(ctx: &egui::Context) {
    crate::fonts::set_fonts(ctx, font_definitions(SystemFonts::default()));
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

/// A grey heading that starts a new section of a page or a dialog: [`SECTION_GAP`] above it
/// (the item gap before it is already there), so it reads as new and sits closer to its own
/// items, which follow at [`ITEM_GAP`]. The first heading at the top of a page or a dialog gets
/// no gap, so there is no empty band above it. Every heading in Settings and in dialogs goes
/// through this (NAV-048).
pub fn section(ui: &mut egui::Ui, title: impl Into<String>) -> egui::Response {
    if !at_top(ui) {
        ui.add_space(SECTION_GAP - ITEM_GAP);
    }
    ui.label(egui::RichText::new(title).color(palette().muted))
}

/// A gap that starts a new block of items (a section without a heading of its own, a step): like
/// the one above a [`section`] heading, none at the top of the `ui`.
pub fn section_break(ui: &mut egui::Ui) {
    if !at_top(ui) {
        ui.add_space(SECTION_GAP - ITEM_GAP);
    }
}

/// Whether nothing has been laid out in `ui` yet.
fn at_top(ui: &egui::Ui) -> bool {
    ui.cursor().min.y <= ui.max_rect().min.y + 0.5
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
    prose(ui, egui::RichText::new(text).color(palette().link_missing))
}

/// A grey 12-point hint or explanation, with [`LINE_GAP`] between its lines.
pub fn hint(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    prose(ui, egui::RichText::new(text).color(palette().muted).size(12.0))
}

/// Run `f` with `palette` as this thread's palette, then put the previous one back. For work that
/// must not follow the theme of the window, such as the PDF export (CFG-061).
pub fn with_palette<T>(palette: Palette, f: impl FnOnce() -> T) -> T {
    struct Restore(Palette);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_palette(self.0);
        }
    }
    let _restore = Restore(self::palette());
    set_palette(palette);
    f()
}

/// Switch the UI thread to `palette` and put it into `ctx`, with Shale's fonts and text sizes
/// (CFG-061). For the start of the app; [`switch_palette`] changes the theme later.
pub fn use_palette(ctx: &egui::Context, palette: Palette) {
    set_palette(palette);
    apply(ctx);
}

/// Change the theme of a running window to `palette` (CFG-062): the palette of this thread and
/// egui's visuals follow; the fonts and text sizes stay as they are.
pub fn switch_palette(ctx: &egui::Context, palette: Palette) {
    set_palette(palette);
    apply_palette(ctx, &palette);
}

/// Put the colours of the current palette and the fonts and sizes of the style into `ctx`.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    set_text_styles(ctx, &AreaFont::shale(crate::fonts::UI_SIZE));
    apply_palette(ctx, &palette());
}

/// Build egui's visuals and style from `p` (CFG-061). The text styles, the animation settings
/// and the cursor's blinking of the style in use stay as they are.
///
/// egui keeps a dark and a light style and, by default, picks one by the system's mode. That
/// would swap our visuals for egui's own whenever the system changes its mode, so the choice is
/// fixed to the one of the palette (CFG-062); the style of the other mode is not used.
pub fn apply_palette(ctx: &egui::Context, p: &Palette) {
    let before = ctx.global_style();
    ctx.set_theme(if p.dark { egui::Theme::Dark } else { egui::Theme::Light });
    // egui would also set the window's own theme (title bar) by it, which hides the system's
    // mode from `Context::system_theme`, and "follow the system" needs that
    ctx.options_mut(|o| o.sync_window_theme = false);
    ctx.set_visuals(if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() });
    let mut style = (*ctx.global_style()).clone();
    style.text_styles = before.text_styles.clone();
    style.animation_time = before.animation_time;
    style.scroll_animation = before.scroll_animation;
    style.visuals.text_cursor.blink = before.visuals.text_cursor.blink;
    // buttons, selectable labels and checkboxes show the pointing hand (LAY-038, #692)
    style.visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.scroll.bar_width = 8.0;
    let v = &mut style.visuals;
    v.panel_fill = p.bg_secondary;
    v.window_fill = p.bg_secondary;
    v.extreme_bg_color = p.bg_primary;
    v.faint_bg_color = p.faint_bg;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.selection.bg_fill = p.selection_bg;
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.hyperlink_color = p.link;
    v.override_text_color = None;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.inactive.bg_fill = p.widget_bg;
    v.widgets.inactive.weak_bg_fill = p.widget_weak_bg;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.inactive.corner_radius = CornerRadius::same(5);
    v.widgets.hovered.bg_fill = p.bg_hover;
    v.widgets.hovered.weak_bg_fill = p.bg_hover;
    v.widgets.hovered.corner_radius = CornerRadius::same(5);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.hover_stroke);
    v.widgets.active.bg_fill = p.bg_active;
    v.widgets.active.weak_bg_fill = p.bg_active;
    v.widgets.active.corner_radius = CornerRadius::same(5);
    v.widgets.open.corner_radius = CornerRadius::same(5);
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);
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

    // spec: CFG-061
    #[test]
    fn cfg_061_shale_has_the_colours_of_before() {
        let p = Palette::shale();
        assert!(p.dark);
        assert_eq!(p.bg_primary, Color32::from_rgb(30, 30, 30));
        assert_eq!(p.bg_secondary, Color32::from_rgb(38, 38, 38));
        assert_eq!(p.text, Color32::from_rgb(218, 218, 218));
        assert_eq!(p.accent, Color32::from_rgb(138, 124, 247));
        assert_eq!(p.link_missing, Color32::from_rgb(255, 104, 104));
        assert_eq!(p.code_bg, Color32::from_rgb(42, 42, 46));
        assert_eq!(p.code, Color32::from_rgb(230, 170, 120));
        assert_eq!(p.highlight_bg, Color32::from_rgba_unmultiplied(200, 170, 40, 60));
        use crate::syntax::Kind;
        let want = [
            (Kind::Keyword, (198, 142, 235)),
            (Kind::String, (152, 205, 120)),
            (Kind::Number, (240, 150, 100)),
            (Kind::Constant, (240, 150, 100)),
            (Kind::Comment, (120, 128, 140)),
            (Kind::Function, (110, 175, 250)),
            (Kind::Type, (230, 200, 110)),
            (Kind::Operator, (110, 205, 210)),
        ];
        for (kind, (r, g, b)) in want {
            assert_eq!(p.syntax(kind), Color32::from_rgb(r, g, b), "{kind:?}");
        }
    }

    // spec: CFG-062
    #[test]
    fn cfg_062_the_generation_counts_the_changes_of_the_palette() {
        let start = generation();
        set_palette(palette());
        assert_eq!(generation(), start, "the same palette is no change");
        let mut other = palette();
        other.text = Color32::from_rgb(9, 8, 7);
        set_palette(other);
        assert_eq!(generation(), start + 1);
        assert_eq!(std::thread::spawn(generation).join().expect("thread"), 0, "each thread counts its own palette");
    }

    // spec: CFG-061
    #[test]
    fn cfg_061_the_palette_is_per_thread_and_defaults_to_shale() {
        assert_eq!(palette(), Palette::shale());
        let mut other = Palette::shale();
        other.text = Color32::from_rgb(1, 2, 3);
        set_palette(other);
        assert_eq!(palette().text, Color32::from_rgb(1, 2, 3));
        let seen = std::thread::spawn(palette).join().expect("thread");
        assert_eq!(seen, Palette::shale(), "another thread keeps its own palette");
        std::thread::spawn(|| set_palette(Palette { accent: Color32::RED, ..Palette::shale() })).join().expect("thread");
        assert_eq!(palette(), other, "and a change there does not reach this one");
        let inside = with_palette(Palette::shale(), palette);
        assert_eq!(inside, Palette::shale());
        assert_eq!(palette(), other, "with_palette puts the palette back");
        set_palette(Palette::shale());
    }

    // spec: CFG-061
    #[test]
    fn cfg_061_apply_palette_builds_the_visuals_of_the_palette() {
        let ctx = Context::default();
        apply_palette(&ctx, &Palette::shale());
        let v = ctx.global_style().visuals.clone();
        let p = Palette::shale();
        assert!(v.dark_mode);
        assert_eq!(v.panel_fill, Color32::from_rgb(38, 38, 38));
        assert_eq!(v.window_fill, Color32::from_rgb(38, 38, 38));
        assert_eq!(v.extreme_bg_color, Color32::from_rgb(30, 30, 30));
        assert_eq!(v.faint_bg_color, Color32::from_rgb(36, 36, 36));
        assert_eq!(v.selection.bg_fill, Color32::from_rgb(70, 64, 130));
        assert_eq!(v.selection.stroke, Stroke::new(1.0, p.accent));
        assert_eq!(v.hyperlink_color, p.link);
        assert_eq!(v.widgets.inactive.bg_fill, Color32::from_rgb(48, 48, 48));
        assert_eq!(v.widgets.inactive.weak_bg_fill, Color32::from_rgb(44, 44, 44));
        assert_eq!(v.widgets.hovered.bg_fill, p.bg_hover);
        assert_eq!(v.widgets.hovered.bg_stroke, Stroke::new(1.0, Color32::from_rgb(70, 70, 70)));
        assert_eq!(v.widgets.active.bg_fill, p.bg_active);
        let light = Palette { dark: false, bg_secondary: Color32::from_rgb(250, 250, 250), ..Palette::shale() };
        apply_palette(&ctx, &light);
        let v = ctx.global_style().visuals.clone();
        assert!(!v.dark_mode, "a palette that is not dark builds on egui's light visuals");
        assert_eq!(v.panel_fill, Color32::from_rgb(250, 250, 250));
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

    // spec: NAV-048
    #[test]
    fn nav_048_a_section_heading_has_the_section_gap_above_it_and_none_at_the_top() {
        in_ui(300.0, |ui| {
            dialog_spacing(ui);
            let first = section(ui, "First");
            assert!(first.rect.top() - ui.max_rect().top() < 0.6, "no empty band above the first heading: {}", first.rect.top() - ui.max_rect().top());
            let item = ui.label("An item");
            let heading = section(ui, "Second");
            let gap = heading.rect.top() - item.rect.bottom();
            assert!((gap - SECTION_GAP).abs() < 0.6, "the gap above a later heading: {gap} of {SECTION_GAP}");
            let next = ui.label("Its item");
            assert!((next.rect.top() - heading.rect.bottom() - ITEM_GAP).abs() < 0.6, "the heading is ITEM_GAP above its own items");
            let before = ui.cursor().min.y;
            section_break(ui);
            assert!((ui.cursor().min.y - before - (SECTION_GAP - ITEM_GAP)).abs() < 0.6);
            ui.scope(|ui| {
                let at = ui.cursor().min.y;
                section_break(ui);
                assert_eq!(ui.cursor().min.y, at, "none at the top of a new ui");
            });
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
            let rich = || RichText::new(LONG).color(palette().muted).size(12.0);
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
            let plain = ui.label(RichText::new(LONG).color(palette().muted).size(12.0)).rect.height();
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
