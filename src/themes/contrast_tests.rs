//! The colours that are really drawn, pair by pair, in every built-in theme (CFG-082, #691): each
//! foreground has the contrast WCAG asks for on each background it is painted on.

use super::mapping::{contrast, over};
use super::{palette_of, Themes};
use crate::theme::{self, Palette};
use eframe::egui::{self, Color32};

/// The contrast of text (WCAG AA) and of large text and parts of the UI.
const TEXT: f32 = 4.5;
const UI: f32 = 3.0;

/// One drawn pair: what it is, the foreground, the background and the contrast it needs.
type Pair = (String, Color32, Color32, f32);

/// Every foreground/background pair that the app paints, taken from `p`.
fn drawn_pairs(p: &Palette) -> Vec<Pair> {
    let mut v: Vec<Pair> = vec![];
    let mut add = |name: &str, fg: Color32, bg: Color32, min: f32| v.push((name.to_string(), fg, bg, min));
    let surfaces = [
        ("bg_primary", p.bg_primary),
        ("bg_secondary", p.bg_secondary),
        ("bg_ribbon", p.bg_ribbon),
        ("faint_bg", p.faint_bg),
        ("code_bg", p.code_bg),
        ("widget_bg", p.widget_bg),
        ("widget_weak_bg", p.widget_weak_bg),
        ("bg_hover", p.bg_hover),
        ("bg_active", p.bg_active),
        ("selection_bg", p.selection_bg),
        ("tag_bg", p.tag_bg),
        ("banner_error_bg", p.banner_error_bg),
        ("banner_notice_bg", p.banner_notice_bg),
        ("good_bg", p.good_bg),
        ("warn_bg", p.warn_bg),
        ("bad_bg", p.bad_bg),
    ];
    for (name, bg) in surfaces {
        add(&format!("text/{name}"), p.text, bg, TEXT);
    }
    for (name, bg) in [("bg_primary", p.bg_primary), ("bg_secondary", p.bg_secondary)] {
        add(&format!("text_strong/{name}"), p.text_strong, bg, TEXT);
        add(&format!("heading/{name}"), p.heading, bg, TEXT);
        add(&format!("muted/{name}"), p.muted, bg, TEXT);
        add(&format!("link/{name}"), p.link, bg, TEXT);
        add(&format!("link_missing/{name}"), p.link_missing, bg, TEXT);
        add(&format!("tag/{name}"), p.tag, bg, TEXT);
        add(&format!("accent/{name}"), p.accent, bg, UI);
        add(&format!("faint/{name}"), p.faint, bg, UI);
        add(&format!("done/{name}"), p.done, bg, UI);
        add(&format!("quote/{name}"), p.quote, bg, UI);
        add(&format!("ok_text/{name}"), p.ok_text, bg, TEXT);
    }
    add("muted/code_bg", p.muted, p.code_bg, TEXT);
    add("tag/tag_bg", p.tag, p.tag_bg, TEXT);
    add("code/code_bg", p.code, p.code_bg, TEXT);
    add("link/code_bg", p.link, p.code_bg, TEXT);
    add("good/good_bg", p.good, p.good_bg, TEXT);
    add("warn/warn_bg", p.warn, p.warn_bg, TEXT);
    add("link_missing/bad_bg", p.link_missing, p.bad_bg, TEXT);
    add("link_missing/banner_error_bg", p.link_missing, p.banner_error_bg, TEXT);
    add("text/highlight_bg", p.text, over(p.highlight_bg, p.bg_primary), TEXT);
    for (name, c) in [("keyword", p.syntax_keyword), ("string", p.syntax_string), ("number", p.syntax_number), ("comment", p.syntax_comment), ("function", p.syntax_function), ("type", p.syntax_type), ("operator", p.syntax_operator)] {
        add(&format!("syntax_{name}/code_bg"), c, p.code_bg, TEXT);
        add(&format!("syntax_{name}/bg_primary"), c, p.bg_primary, TEXT);
    }
    v
}

/// The built-in themes, Shale's own among them.
pub(super) fn themes() -> Vec<(String, Palette)> {
    let t = Themes::load(None);
    assert!(t.list.len() >= 9, "Shale and the eight files");
    t.list.into_iter().map(|t| (t.name, t.palette)).collect()
}

/// The pairs that fail in `p`, as lines of text.
fn failures(p: &Palette) -> Vec<String> {
    drawn_pairs(p).into_iter().filter(|(_, fg, bg, min)| contrast(*fg, *bg) < *min).map(|(n, fg, bg, min)| format!("{n}: {:.2} < {min} ({fg:?} on {bg:?})", contrast(fg, bg))).collect()
}

// spec: CFG-082
#[test]
fn cfg_082_every_drawn_pair_of_every_builtin_theme_has_the_contrast_of_wcag() {
    let mut all = vec![];
    for (name, p) in themes() {
        all.extend(failures(&p).into_iter().map(|f| format!("{name}: {f}")));
    }
    assert!(all.is_empty(), "{} pairs fail:\n{}", all.len(), all.join("\n"));
}

/// The visuals egui draws with for `p`, as the pairs of what egui paints with them.
fn visual_pairs(p: &Palette) -> Vec<Pair> {
    let ctx = egui::Context::default();
    theme::apply_palette(&ctx, p);
    let v = ctx.global_style().visuals.clone();
    let mut out: Vec<Pair> = vec![("selected row".into(), v.selection.stroke.color, v.selection.bg_fill, TEXT)];
    for (name, w) in [("noninteractive", &v.widgets.noninteractive), ("inactive", &v.widgets.inactive), ("hovered", &v.widgets.hovered), ("active", &v.widgets.active), ("open", &v.widgets.open)] {
        out.push((format!("{name} on bg_fill"), w.fg_stroke.color, w.bg_fill, TEXT));
        out.push((format!("{name} on weak_bg_fill"), w.fg_stroke.color, w.weak_bg_fill, TEXT));
    }
    out.push(("noninteractive on the panel".into(), v.widgets.noninteractive.fg_stroke.color, v.panel_fill, TEXT));
    out.push(("noninteractive on the field".into(), v.widgets.noninteractive.fg_stroke.color, v.extreme_bg_color, TEXT));
    out.push(("hyperlink on the panel".into(), v.hyperlink_color, v.panel_fill, TEXT));
    out
}

// spec: CFG-082
#[test]
fn cfg_082_the_text_egui_draws_on_a_selected_hovered_or_active_widget_reads_in_every_theme() {
    let mut bad = vec![];
    for (name, p) in themes() {
        for (what, fg, bg, min) in visual_pairs(&p) {
            if contrast(fg, bg) < min {
                bad.push(format!("{name}: {what}: {:.2} < {min} ({fg:?} on {bg:?})", contrast(fg, bg)));
            }
        }
    }
    assert!(bad.is_empty(), "{} pairs fail:\n{}", bad.len(), bad.join("\n"));
}

// spec: CFG-082
#[test]
fn cfg_082_a_user_theme_gets_the_same_derivation() {
    // a loud terminal theme: pastel colours on a light background, a hot selection
    let text = "[colors.primary]\nbackground = '#f6f0f0'\nforeground = '#6a5a6a'\n[colors.normal]\nblack = '#000000'\nred = '#ff9090'\ngreen = '#a0ff90'\nyellow = '#ffff90'\nblue = '#90a0ff'\nmagenta = '#ff80c0'\ncyan = '#90ffff'\nwhite = '#ffffff'\n[colors.selection]\nbackground = '#ffb0b0'\n";
    let p = palette_of(&super::format::parse(text).unwrap());
    assert_eq!(failures(&p), Vec::<String>::new());
    assert!(visual_pairs(&p).iter().all(|(_, fg, bg, min)| contrast(*fg, *bg) >= *min));
}
