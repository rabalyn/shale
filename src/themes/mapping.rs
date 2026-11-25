//! From the colours of a terminal theme to a Shale palette (CFG-064).

use super::format::{ThemeFile, BLACK, BLUE, CYAN, GREEN, MAGENTA, RED, YELLOW};
use crate::theme::Palette;
use eframe::egui::Color32;

/// The mix of `a` and `b`: `t` of 0 is `a`, 1 is `b`, linear in each channel.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

/// The relative luminance of WCAG 2: 0 for black, 1 for white.
pub fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// The contrast ratio of WCAG 2 between two colours, 1 to 21.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `c`, moved towards `toward` in steps of a twentieth until its contrast against `bg` is at
/// least `min` (it ends at `toward` when that never happens).
pub fn lift(c: Color32, bg: Color32, toward: Color32, min: f32) -> Color32 {
    (0..=20).map(|step| mix(c, toward, step as f32 / 20.0)).find(|x| contrast(*x, bg) >= min).unwrap_or(toward)
}

/// The share of foreground in each background colour of the window, from the palette of Shale
/// (CFG-064): the background of the window is `mix(bg, fg, t)`.
const RIBBON: f32 = 0.016;
const FAINT_BG: f32 = 0.032;
const SECONDARY: f32 = 0.043;
const CODE_BG: f32 = 0.064;
const WIDGET_WEAK: f32 = 0.074;
const WIDGET: f32 = 0.096;
const HOVER: f32 = 0.117;
const BORDER: f32 = 0.128;
const HOVER_STROKE: f32 = 0.21;
/// The same for the greys of text.
const FAINT: f32 = 0.40;
const DONE: f32 = 0.48;
const MUTED: f32 = 0.64;
const QUOTE: f32 = 0.75;
/// Strong text and headings: how far the text colour goes towards white (dark) or black (light).
const STRONG: f32 = 0.6;
const HEADING: f32 = 0.5;
/// The share of a colour in the background of what it marks.
const ACTIVE: f32 = 0.22;
const TAG_BG: f32 = 0.2;
const SELECTION: f32 = 0.4;
const BANNER: f32 = 0.17;
const STATUS_BG: f32 = 0.15;
/// The greys of the graph's lines.
const EDGE_DIM: f32 = 0.11;
const EDGE: f32 = 0.26;
/// The lowest contrast against the background: text, and everything else that is read.
pub const TEXT_CONTRAST: f32 = 4.5;
pub const READ_CONTRAST: f32 = 3.0;
/// The opacity of the highlight of `==text==` (0 to 255).
const HIGHLIGHT_ALPHA: u8 = 60;

/// `top`, with its alpha, painted over the opaque `under`.
pub fn over(top: Color32, under: Color32) -> Color32 {
    mix(under, Color32::from_rgb(top.r(), top.g(), top.b()), f32::from(top.a()) / 255.0)
}

/// `c`, moved towards `toward` in steps of a twentieth until its contrast against every one of
/// `bgs` is at least `min` (it ends at `toward` when that never happens).
pub fn lift_all(c: Color32, bgs: &[Color32], toward: Color32, min: f32) -> Color32 {
    (0..=20).map(|step| mix(c, toward, step as f32 / 20.0)).find(|x| bgs.iter().all(|bg| contrast(*x, *bg) >= min)).unwrap_or(toward)
}

/// The palette of a theme file: the mapping of CFG-064, then the slots of its `[shale]` table.
pub fn palette_of(file: &ThemeFile) -> Palette {
    let t = &file.terminal;
    let (bg, fg) = (t.background, t.foreground);
    let dark = file.dark.unwrap_or(luminance(bg) < 0.5);
    let (extreme, n, b) = (if dark { Color32::WHITE } else { Color32::BLACK }, &t.normal, &t.bright);
    let text = lift(fg, bg, extreme, TEXT_CONTRAST);
    // a background that is `share` of the way from `bg` to `to`, with less of `to` (in steps of a
    // twentieth of the share) until the text reads on it (CFG-082)
    let tint = |to: Color32, share: f32| (0..=20).map(|k| mix(bg, to, share * (1.0 - k as f32 / 20.0))).find(|c| contrast(text, *c) >= TEXT_CONTRAST).unwrap_or(bg);
    let (secondary, code_bg) = (tint(fg, SECONDARY), tint(fg, CODE_BG));
    let (surfaces, panels) = ([bg, secondary, code_bg], [bg, secondary]);
    // a colour that is read as text on the surfaces of the window (and on `more`), moved towards
    // the extreme until it has `min` on all of them
    let read_on = |c: Color32, more: &[Color32], min: f32| lift_all(c, &[&surfaces[..], more].concat(), extreme, min);
    let read = |c: Color32| read_on(c, &[], TEXT_CONTRAST);
    // the greys that only have to be told from the background (not read at length)
    let grey = |c: Color32| lift_all(c, &panels, extreme, READ_CONTRAST);
    let accent = lift_all(n[MAGENTA], &panels, extreme, READ_CONTRAST);
    let link = if contrast(b[BLUE], bg) > contrast(n[BLUE], bg) { b[BLUE] } else { n[BLUE] };
    // the selection of the file when the text can be read on it, else the accent in the background,
    // as much of it as the text still reads on
    let selection = t.selection.filter(|s| contrast(*s, text) >= TEXT_CONTRAST).unwrap_or_else(|| tint(accent, SELECTION));
    let (tag_bg, banner_error_bg, bad_bg, good_bg, warn_bg) = (tint(accent, TAG_BG), tint(n[RED], BANNER), tint(n[RED], STATUS_BG), tint(n[GREEN], STATUS_BG), tint(n[YELLOW], STATUS_BG));
    let green = read_on(n[GREEN], &[good_bg], TEXT_CONTRAST);
    let yellow = read_on(n[YELLOW], &[warn_bg], TEXT_CONTRAST);
    let link_missing = read_on(n[RED], &[bad_bg, banner_error_bg], TEXT_CONTRAST);
    // the highlight is less opaque when the text would not read through it
    let highlight = |a: u8| Color32::from_rgba_unmultiplied(n[YELLOW].r(), n[YELLOW].g(), n[YELLOW].b(), a);
    let highlight_bg = (0..=HIGHLIGHT_ALPHA).rev().step_by(5).map(highlight).find(|h| contrast(text, over(*h, bg)) >= TEXT_CONTRAST).unwrap_or_else(|| highlight(0));
    let number = if b[RED] == n[RED] { n[YELLOW] } else { b[RED] };
    let m = |share: f32| mix(bg, fg, share);
    let mut p = Palette {
        dark,
        bg_primary: bg,
        bg_secondary: secondary,
        bg_ribbon: tint(fg, RIBBON),
        bg_hover: tint(fg, HOVER),
        bg_active: tint(accent, ACTIVE),
        border: m(BORDER),
        text,
        text_strong: lift(mix(fg, extreme, STRONG), bg, extreme, TEXT_CONTRAST),
        muted: read(m(MUTED)),
        faint: grey(m(FAINT)),
        accent,
        link: read(link),
        link_missing,
        tag: read_on(b[MAGENTA], &[tag_bg], TEXT_CONTRAST),
        tag_bg,
        code_bg,
        code: read(n[YELLOW]),
        heading: lift(mix(fg, extreme, HEADING), bg, extreme, TEXT_CONTRAST),
        done: grey(m(DONE)),
        quote: grey(m(QUOTE)),
        syntax_keyword: read(n[MAGENTA]),
        syntax_string: read(n[GREEN]),
        syntax_number: read(number),
        syntax_comment: read(b[BLACK]),
        syntax_function: read(n[BLUE]),
        syntax_type: read(n[YELLOW]),
        syntax_operator: read(n[CYAN]),
        faint_bg: tint(fg, FAINT_BG),
        selection_bg: selection,
        widget_bg: tint(fg, WIDGET),
        widget_weak_bg: tint(fg, WIDGET_WEAK),
        hover_stroke: m(HOVER_STROKE),
        highlight_bg,
        banner_error_bg,
        banner_notice_bg: tint(accent, BANNER),
        ok_text: green,
        good: green,
        good_bg,
        warn: yellow,
        warn_bg,
        bad_bg,
        graph_edge_dim: m(EDGE_DIM),
        graph_edge: m(EDGE),
        graph_node: m(QUOTE),
        graph_node_drawing: green,
    };
    // the author asked for these: no correction
    for (name, colour) in &file.slots {
        if let Some(slot) = p.slot_mut(name) {
            *slot = *colour;
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::format::parse;

    fn rgb(r: u8, g: u8, b: u8) -> Color32 {
        Color32::from_rgb(r, g, b)
    }

    /// A terminal theme file with these two colours and a plain set of colours.
    fn file(bg: &str, fg: &str, extra: &str) -> ThemeFile {
        let text = format!(
            "[colors.primary]\nbackground = '{bg}'\nforeground = '{fg}'\n[colors.normal]\nblack = '#000000'\nred = '#ff6666'\ngreen = '#55dd55'\nyellow = '#dddd55'\nblue = '#6699ee'\nmagenta = '#dd66dd'\ncyan = '#55dddd'\nwhite = '#cccccc'\n{extra}"
        );
        parse(&text).unwrap()
    }

    // spec: CFG-064
    #[test]
    fn cfg_064_mix_is_linear_per_channel_and_contrast_is_wcag() {
        assert_eq!(mix(rgb(0, 100, 255), rgb(100, 200, 55), 0.0), rgb(0, 100, 255));
        assert_eq!(mix(rgb(0, 100, 255), rgb(100, 200, 55), 1.0), rgb(100, 200, 55));
        assert_eq!(mix(rgb(0, 100, 255), rgb(100, 200, 55), 0.5), rgb(50, 150, 155));
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
        assert!((contrast(rgb(119, 119, 119), Color32::WHITE) - 4.48).abs() < 0.05, "#777 on white is the known 4.48");
        assert!((contrast(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 1e-6);
        assert!(luminance(rgb(30, 30, 30)) < 0.5 && luminance(rgb(253, 246, 227)) > 0.5);
    }

    // spec: CFG-064
    #[test]
    fn cfg_064_a_terminal_theme_in_the_colours_of_shale_comes_out_close_to_shale() {
        let shale = Palette::shale();
        let p = palette_of(&file("#1e1e1e", "#dadada", ""));
        assert!(p.dark);
        assert_eq!((p.bg_primary, p.text), (shale.bg_primary, shale.text));
        // the greys of the window: within 3 of 255 per channel (the code background of Shale is a little blue)
        let near = |a: Color32, b: Color32, what: &str| {
            let d = a.r().abs_diff(b.r()).max(a.g().abs_diff(b.g())).max(a.b().abs_diff(b.b()));
            assert!(d <= if what == "code_bg" { 4 } else { 3 }, "{what}: {a:?} against {b:?}");
        };
        near(p.bg_secondary, shale.bg_secondary, "bg_secondary");
        near(p.bg_ribbon, shale.bg_ribbon, "bg_ribbon");
        near(p.bg_hover, shale.bg_hover, "bg_hover");
        near(p.border, shale.border, "border");
        near(p.faint_bg, shale.faint_bg, "faint_bg");
        near(p.widget_bg, shale.widget_bg, "widget_bg");
        near(p.widget_weak_bg, shale.widget_weak_bg, "widget_weak_bg");
        near(p.hover_stroke, shale.hover_stroke, "hover_stroke");
        near(p.muted, shale.muted, "muted");
        near(p.faint, shale.faint, "faint");
        near(p.done, shale.done, "done");
        near(p.quote, shale.quote, "quote");
        near(p.text_strong, shale.text_strong, "text_strong");
        near(p.heading, shale.heading, "heading");
        near(p.code_bg, shale.code_bg, "code_bg");
        near(p.graph_edge_dim, shale.graph_edge_dim, "graph_edge_dim");
        near(p.graph_edge, shale.graph_edge, "graph_edge");
        near(p.graph_node, shale.graph_node, "graph_node");
    }

    // spec: CFG-064
    #[test]
    fn cfg_064_the_colours_come_from_the_named_terminal_colours() {
        let p = palette_of(&file("#1e1e1e", "#dadada", "[colors.bright]\nmagenta = '#ff55ff'\nred = '#ff5555'\nblack = '#9a9a9a'\n"));
        assert_eq!(p.accent, rgb(0xdd, 0x66, 0xdd), "normal magenta");
        assert_eq!(p.tag, rgb(0xff, 0x55, 0xff), "bright magenta");
        assert_eq!(p.link, rgb(0x66, 0x99, 0xee), "normal blue (its contrast is lifted only when it is under 4.5)");
        assert_eq!(p.link_missing, rgb(0xff, 0x66, 0x66));
        assert_eq!(p.code, rgb(0xdd, 0xdd, 0x55));
        assert_eq!(p.syntax_keyword, rgb(0xdd, 0x66, 0xdd));
        assert_eq!(p.syntax_string, rgb(0x55, 0xdd, 0x55));
        assert_eq!(p.syntax_number, rgb(0xff, 0x55, 0x55), "bright red");
        assert_eq!(p.syntax_comment, rgb(0x9a, 0x9a, 0x9a), "bright black");
        assert_eq!(p.syntax_operator, rgb(0x55, 0xdd, 0xdd));
        assert_eq!(p.good, p.syntax_string);
        assert_eq!(p.highlight_bg, Color32::from_rgba_unmultiplied(0xdd, 0xdd, 0x55, 60));
        assert_eq!(p.bg_active, mix(p.bg_primary, p.accent, ACTIVE));
        assert_eq!(p.tag_bg, mix(p.bg_primary, p.accent, TAG_BG));
        // without a bright red that differs, a number is yellow, so it does not look like a problem
        assert_eq!(palette_of(&file("#1e1e1e", "#dadada", "")).syntax_number, rgb(0xdd, 0xdd, 0x55));
        // the link takes the bright blue when its contrast is better
        let bright_blue = palette_of(&file("#1e1e1e", "#dadada", "[colors.bright]\nblue = '#99bbff'\n"));
        assert_eq!(bright_blue.link, rgb(0x99, 0xbb, 0xff));
    }

    // spec: CFG-064
    #[test]
    fn cfg_064_dark_is_found_by_the_luminance_of_the_background_and_light_bases_on_black_text() {
        let dark = palette_of(&file("#282a36", "#f8f8f2", ""));
        let light = palette_of(&file("#fdf6e3", "#586e75", ""));
        assert!(dark.dark && !light.dark);
        assert!(luminance(dark.text_strong) > luminance(dark.text), "stronger text is lighter on dark");
        assert!(luminance(light.text_strong) < luminance(light.text), "and darker on light");
        assert!(luminance(light.bg_hover) < luminance(light.bg_primary), "a hover is darker than a light window");
        assert!(luminance(dark.bg_hover) > luminance(dark.bg_primary), "and lighter than a dark one");
        // `[shale] dark` decides when the colours are not what the luminance says
        assert!(!palette_of(&file("#282a36", "#f8f8f2", "[shale]\ndark = false\n")).dark);
        assert!(palette_of(&file("#fdf6e3", "#586e75", "[shale]\ndark = true\n")).dark);
        // a selection colour that the text could not be read on is not used
        let cream = palette_of(&file("#1e1e1e", "#dadada", "[colors.selection]\nbackground = '#f5e0dc'\n"));
        assert_eq!(cream.selection_bg, mix(cream.bg_primary, cream.accent, SELECTION));
        // and so is a made one that would be too bright for the text of a light theme
        let pale = palette_of(&file("#fdf6e3", "#586e75", ""));
        assert!(contrast(pale.selection_bg, pale.text) >= TEXT_CONTRAST, "{:?}", pale.selection_bg);
        assert_ne!(pale.selection_bg, pale.bg_primary);
        let own = palette_of(&file("#1e1e1e", "#dadada", "[colors.selection]\nbackground = '#334455'\n"));
        assert_eq!(own.selection_bg, rgb(0x33, 0x44, 0x55));
    }

    // spec: CFG-064
    #[test]
    fn cfg_064_a_colour_that_is_too_faint_is_lifted_towards_the_text_until_it_reads() {
        // a bright black that is nearly the background: the comment would vanish
        let p = palette_of(&file("#1e1e1e", "#dadada", "[colors.bright]\nblack = '#222222'\n"));
        assert!(contrast(p.syntax_comment, p.bg_primary) >= TEXT_CONTRAST, "{:?}", p.syntax_comment);
        assert!(luminance(p.syntax_comment) > luminance(rgb(0x22, 0x22, 0x22)), "it moved towards the text");
        // on a light background the colours are the same kind of thing, moved towards the dark text
        let light = palette_of(&file("#ffffff", "#222222", ""));
        assert!(light.syntax_type.g() < 0xcc, "a light yellow got darker: {:?}", light.syntax_type);
        for c in [light.faint, light.done, light.quote] {
            assert!(contrast(c, light.bg_primary) >= READ_CONTRAST, "{c:?}");
        }
        for c in [light.muted, light.link, light.link_missing, light.syntax_comment, light.syntax_string, light.syntax_type, light.text, light.text_strong, light.heading] {
            assert!(contrast(c, light.bg_primary) >= TEXT_CONTRAST, "{c:?}");
        }
        // text that is too faint moves towards white or black
        let dim = palette_of(&file("#202020", "#303030", ""));
        assert!(contrast(dim.text, dim.bg_primary) >= TEXT_CONTRAST);
    }

    // spec: CFG-064
    #[test]
    fn cfg_064_the_shale_table_wins_and_is_not_corrected() {
        let p = palette_of(&file("#1e1e1e", "#dadada", "[shale]\nsyntax_comment = '#1f1f1f'\naccent = '#123456'\nhighlight_bg = '#ff000080'\n"));
        assert_eq!(p.syntax_comment, rgb(0x1f, 0x1f, 0x1f), "even a colour that cannot be read");
        assert!(contrast(p.syntax_comment, p.bg_primary) < READ_CONTRAST);
        assert_eq!(p.accent, rgb(0x12, 0x34, 0x56));
        assert_eq!(p.highlight_bg, Color32::from_rgba_unmultiplied(255, 0, 0, 0x80));
        assert_eq!(p.bg_active, palette_of(&file("#1e1e1e", "#dadada", "")).bg_active, "what is made from the accent is not remade");
    }
}
