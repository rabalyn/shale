//! The formatting toolbar above the text of a note (EDIT-026): a row of buttons that returns the
//! command a click asks for. What the commands do to the text is `format.rs`.

use crate::format::{self, Cmd};
use crate::key_badges::Badges;
use crate::keys::Action;
use crate::theme;
use eframe::egui::{self, text::LayoutJob, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke, TextFormat, Ui, Vec2};
use std::ops::Range;

/// The colours the palettes offer (the settings, EDIT-028) and the size of the buttons.
pub struct Palettes<'a> {
    /// the Alt keys to show on the buttons while Alt is held (KEY-016)
    pub badges: &'a Badges,
    pub highlight: &'a [[u8; 3]],
    pub font: &'a [[u8; 3]],
    /// the buttons' size as a factor of the base size (the setting "Toolbar button size", EDIT-026)
    pub scale: f32,
}

/// A button with a popup that a key opens (KEY-015).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Menu {
    Highlight,
    Font,
    Heading,
}

/// The buttons that run a command, with their Alt keys (KEY-015).
pub const BUTTON_KEYS: [(Action, Cmd); 16] = [
    (Action::NoteBold, Cmd::Bold),
    (Action::NoteItalic, Cmd::Italic),
    (Action::NoteStrike, Cmd::Strike),
    (Action::NoteHighlight, Cmd::Highlight),
    (Action::NoteCode, Cmd::Code),
    (Action::NoteBullet, Cmd::Bullet),
    (Action::NoteNumbered, Cmd::Numbered),
    (Action::NoteTask, Cmd::Task),
    (Action::NoteQuote, Cmd::Quote),
    (Action::NoteCodeBlock, Cmd::CodeBlock),
    (Action::NoteRule, Cmd::Rule),
    (Action::NoteTable, Cmd::Table),
    (Action::NoteWikiLink, Cmd::WikiLink),
    (Action::NoteWebLink, Cmd::WebLink),
    (Action::NoteEmbed, Cmd::Embed),
    (Action::NoteClear, Cmd::Clear),
];

/// The command a key of the note view runs, for the buttons that run one.
pub fn cmd_of(a: Action) -> Option<Cmd> {
    BUTTON_KEYS.iter().find(|(k, _)| *k == a).map(|(_, c)| *c)
}

/// Whether a button for `cmd` can be pressed in a note (`cell`: in a table cell, EDIT-030): the rule of its look and its key.
pub fn enabled_in(cmd: Cmd, cell: bool) -> bool {
    !cell || format::works_in_a_cell(cmd)
}

/// The toolbar's own state: the colours of the pickers and which of them is open.
pub struct FormatBar {
    /// a popup to open in the next frame (the key of its button, KEY-015)
    open_request: Option<Menu>,
    custom_mark: Color32,
    custom_font: Color32,
    /// the colours last used, shown on the two colour buttons
    last_mark: [u8; 3],
    last_font: [u8; 3],
    picker_open: [bool; 2],
    /// the width the bar took last frame: it is drawn centred, and its width is known only once drawn
    width: f32,
}

impl Default for FormatBar {
    fn default() -> Self {
        FormatBar { open_request: None, custom_mark: Color32::from_rgb(255, 165, 0), custom_font: Color32::from_rgb(220, 20, 60), last_mark: [255, 255, 0], last_font: [229, 62, 62], picker_open: [false; 2], width: 0.0 }
    }
}

/// How a button's text looks.
#[derive(Clone, Copy, Default)]
struct Label {
    mono: bool,
    bold: bool,
    italic: bool,
    strike: bool,
}

/// How the buttons are drawn: their scale and whether they are for the text of a table cell (EDIT-030).
#[derive(Clone, Copy)]
struct Look<'a> {
    scale: f32,
    cell: bool,
    badges: &'a Badges,
}

impl Look<'_> {
    /// Whether `cmd` can be pressed: all of them for the note, the inline ones in a cell.
    fn enabled(self, cmd: Cmd) -> bool {
        enabled_in(cmd, self.cell)
    }

    /// The Alt key of the button for `cmd` at its corner, while Alt is held and the button is enabled (KEY-016).
    fn badge(self, ui: &Ui, rect: Rect, cmd: Cmd) {
        if let Some((action, _)) = BUTTON_KEYS.iter().find(|(_, c)| *c == cmd).filter(|_| self.enabled(cmd)) {
            self.badges.paint(ui, rect, *action);
        }
    }
}

/// The height of a button at scale 1.
const BUTTON: f32 = 26.0;

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// The button's look: the hover and pressed backgrounds of the icon buttons, then `draw` in the
/// colour that goes with the state. Its tooltip is also its accessibility label (and the hint of scripts).
/// `width` and everything `draw` paints are at scale 1; the button and the drawing are `s` times as big.
fn tool(ui: &mut Ui, s: f32, width: f32, on: bool, enabled: bool, tip: &str, draw: impl FnOnce(&egui::Painter, Rect, Color32, f32)) -> Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(width, BUTTON) * s, if enabled { Sense::click() } else { Sense::hover() });
    let hovered = enabled && resp.hovered();
    if on {
        ui.painter().rect_filled(r, CornerRadius::same(5), theme::BG_ACTIVE);
    } else if hovered {
        ui.painter().rect_filled(r, CornerRadius::same(5), theme::BG_HOVER);
    }
    let c = if !enabled {
        theme::FAINT
    } else if on {
        theme::ACCENT
    } else if hovered {
        theme::TEXT_STRONG
    } else {
        theme::TEXT
    };
    draw(ui.painter(), r, c, s);
    crate::script::hint(tip, r);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, tip));
    resp.on_hover_text(tip)
}

fn text_galley(painter: &egui::Painter, text: &str, l: Label, size: f32, c: Color32) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::default();
    let font_id = if l.mono { FontId::monospace(size) } else { FontId::proportional(size) };
    let strike = if l.strike { Stroke::new(1.2, c) } else { Stroke::NONE };
    job.append(text, 0.0, TextFormat { font_id, color: c, italics: l.italic, strikethrough: strike, extra_letter_spacing: if l.bold { 0.4 } else { 0.0 }, ..Default::default() });
    painter.layout_job(job)
}

/// A button with a text on it.
fn text_tool(ui: &mut Ui, s: f32, text: &str, l: Label, on: bool, enabled: bool, tip: &str) -> Response {
    let size = if l.mono { 12.0 } else { 14.5 };
    let w = text_galley(ui.painter(), text, l, size, theme::TEXT).size().x + 12.0;
    tool(ui, s, w.max(BUTTON), on, enabled, tip, |p, r, c, s| {
        let g = text_galley(p, text, l, size * s, c);
        let pos = r.center() - g.size() / 2.0;
        p.galley(pos, g.clone(), c);
        if l.bold {
            p.galley(pos + Vec2::new(0.6 * s, 0.0), g, c);
        }
    })
}

/// A little triangle that says "a menu".
fn caret(p: &egui::Painter, at: Pos2, c: Color32, s: f32) {
    p.add(egui::Shape::convex_polygon(vec![at + Vec2::new(-3.0, -1.5) * s, at + Vec2::new(3.0, -1.5) * s, at + Vec2::new(0.0, 2.0) * s], c, Stroke::NONE));
}

fn draw_bullets(p: &egui::Painter, r: Rect, c: Color32, s: f32) {
    for k in 0..3 {
        let y = r.center().y + (k as f32 - 1.0) * 5.0 * s;
        p.circle_filled(Pos2::new(r.left() + 7.0 * s, y), 1.4 * s, c);
        p.line_segment([Pos2::new(r.left() + 11.0 * s, y), Pos2::new(r.right() - 6.0 * s, y)], Stroke::new(1.3 * s, c));
    }
}

fn draw_task(p: &egui::Painter, r: Rect, c: Color32, s: f32) {
    let b = Rect::from_center_size(r.center(), Vec2::splat(12.0 * s));
    p.rect_stroke(b, 2.0 * s, Stroke::new(1.3 * s, c), egui::StrokeKind::Inside);
    p.line_segment([b.center() + Vec2::new(-3.0, 0.5) * s, b.center() + Vec2::new(-0.8, 2.8) * s], Stroke::new(1.5 * s, c));
    p.line_segment([b.center() + Vec2::new(-0.8, 2.8) * s, b.center() + Vec2::new(3.2, -2.6) * s], Stroke::new(1.5 * s, c));
}

fn draw_quote(p: &egui::Painter, r: Rect, c: Color32, s: f32) {
    let x = r.left() + 8.0 * s;
    p.line_segment([Pos2::new(x, r.center().y - 6.0 * s), Pos2::new(x, r.center().y + 6.0 * s)], Stroke::new(2.0 * s, c));
    for dy in [-3.5 * s, 3.5 * s] {
        p.line_segment([Pos2::new(x + 4.0 * s, r.center().y + dy), Pos2::new(r.right() - 6.0 * s, r.center().y + dy)], Stroke::new(1.3 * s, c));
    }
}

fn draw_rule(p: &egui::Painter, r: Rect, c: Color32, s: f32) {
    p.line_segment([Pos2::new(r.left() + 6.0 * s, r.center().y), Pos2::new(r.right() - 6.0 * s, r.center().y)], Stroke::new(1.6 * s, c));
}

fn draw_table(p: &egui::Painter, r: Rect, c: Color32, s: f32) {
    let b = Rect::from_center_size(r.center(), Vec2::new(14.0, 12.0) * s);
    p.rect_stroke(b, s, Stroke::new(1.2 * s, c), egui::StrokeKind::Inside);
    p.line_segment([Pos2::new(b.left(), b.center().y), Pos2::new(b.right(), b.center().y)], Stroke::new(s, c));
    p.line_segment([Pos2::new(b.center().x, b.top()), Pos2::new(b.center().x, b.bottom())], Stroke::new(s, c));
}

fn draw_clear(p: &egui::Painter, r: Rect, c: Color32, s: f32) {
    let m = r.center();
    p.line_segment([m + Vec2::new(-4.5, -4.5) * s, m + Vec2::new(4.5, 4.5) * s], Stroke::new(1.6 * s, c));
    p.line_segment([m + Vec2::new(-4.5, 4.5) * s, m + Vec2::new(4.5, -4.5) * s], Stroke::new(1.6 * s, c));
}

/// A text button for a command: the command when it was clicked.
fn text_cmd(ui: &mut Ui, b: Look, on: bool, cmd: Cmd, text: &str, l: Label, tip: &str) -> Option<Cmd> {
    let resp = text_tool(ui, b.scale, text, l, on, b.enabled(cmd), tip);
    b.badge(ui, resp.rect, cmd);
    resp.clicked().then_some(cmd)
}

/// A drawn button for a command: the command when it was clicked.
fn draw_cmd(ui: &mut Ui, b: Look, on: bool, cmd: Cmd, tip: &str, draw: fn(&egui::Painter, Rect, Color32, f32)) -> Option<Cmd> {
    let resp = tool(ui, b.scale, BUTTON, on, b.enabled(cmd), tip, draw);
    b.badge(ui, resp.rect, cmd);
    resp.clicked().then_some(cmd)
}

fn separator(ui: &mut Ui, s: f32) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(7.0, BUTTON) * s, Sense::hover());
    ui.painter().line_segment([Pos2::new(r.center().x, r.top() + 5.0 * s), Pos2::new(r.center().x, r.bottom() - 5.0 * s)], Stroke::new(1.0, theme::BORDER));
}

impl FormatBar {
    /// Open the popup of a button in the next frame (its key, KEY-015).
    pub fn open(&mut self, menu: Menu) {
        self.open_request = Some(menu);
    }

    /// Draw the toolbar; returns the command a click asked for. `text` and `sel` (bytes) are the note and
    /// its selection, which say what is "on" at the cursor. `cell`: they are the text of a table cell edited
    /// in the grid (EDIT-030), and the buttons that make no sense there are greyed out.
    pub fn show(&mut self, ui: &mut Ui, text: &str, sel: Range<usize>, cell: bool, palettes: &Palettes) -> Option<Cmd> {
        // centred over the editor (EDIT-026): the space left of it is half of what last frame's bar left
        // free; a bar that wraps (more than one row of buttons) counts as the whole width and so has none
        let full = ui.available_width();
        let pad = ((full - self.width) / 2.0).max(0.0).floor();
        let frame = ui.horizontal_top(|ui| {
            ui.add_space(pad);
            self.frame(ui, text, &sel, cell, palettes)
        });
        let (size, cmd) = frame.inner;
        let width = if size.y > BUTTON * palettes.scale * 1.5 { full } else { size.x };
        if (width - self.width).abs() > 0.5 {
            self.width = width;
            ui.ctx().request_repaint();
        }
        cmd
    }

    /// The bar's frame and buttons; returns the frame's size and the command a click asked for.
    fn frame(&mut self, ui: &mut Ui, text: &str, sel: &Range<usize>, cell: bool, palettes: &Palettes) -> (Vec2, Option<Cmd>) {
        let mut out = None;
        let on = |cmd| format::is_on(cmd, text, sel.clone());
        let open = self.open_request.take();
        let bar = Look { scale: palettes.scale, cell, badges: palettes.badges };
        let sc = bar.scale;
        let resp = egui::Frame::new().fill(theme::BG_SECONDARY).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(1.0, 2.0);
                let mono = Label { mono: true, ..Default::default() };
                let mut cmds: Vec<Option<Cmd>> = vec![
                    text_cmd(ui, bar, on(Cmd::Bold), Cmd::Bold, "B", Label { bold: true, ..Default::default() }, "Bold: **text**"),
                    text_cmd(ui, bar, on(Cmd::Italic), Cmd::Italic, "I", Label { italic: true, ..Default::default() }, "Italic: *text*"),
                    text_cmd(ui, bar, on(Cmd::Strike), Cmd::Strike, "S", Label { strike: true, ..Default::default() }, "Strikethrough: ~~text~~"),
                    text_cmd(ui, bar, on(Cmd::Highlight), Cmd::Highlight, "==", mono, "Highlight: ==text=="),
                    text_cmd(ui, bar, on(Cmd::Code), Cmd::Code, "<>", mono, "Inline code: `code`"),
                ];
                separator(ui, sc);
                cmds.push(self.palette(ui, 0, palettes, open == Some(Menu::Highlight)));
                cmds.push(self.palette(ui, 1, palettes, open == Some(Menu::Font)));
                separator(ui, sc);
                cmds.push(heading_menu(ui, bar, text, sel, open == Some(Menu::Heading)));
                cmds.push(draw_cmd(ui, bar, on(Cmd::Bullet), Cmd::Bullet, "Bullet list: - item", draw_bullets));
                cmds.push(text_cmd(ui, bar, on(Cmd::Numbered), Cmd::Numbered, "1.", Label::default(), "Numbered list: 1. item"));
                cmds.push(draw_cmd(ui, bar, on(Cmd::Task), Cmd::Task, "Task: - [ ] item", draw_task));
                cmds.push(draw_cmd(ui, bar, on(Cmd::Quote), Cmd::Quote, "Quote: > text", draw_quote));
                separator(ui, sc);
                cmds.push(text_cmd(ui, bar, false, Cmd::CodeBlock, "{ }", mono, "Code block: ``` fence"));
                cmds.push(draw_cmd(ui, bar, false, Cmd::Rule, "Horizontal rule: ---", draw_rule));
                cmds.push(draw_cmd(ui, bar, false, Cmd::Table, "Table: 2 columns, 2 rows", draw_table));
                separator(ui, sc);
                cmds.push(text_cmd(ui, bar, false, Cmd::WikiLink, "[[ ]]", mono, "Link to a note: [[Note]]"));
                cmds.push(text_cmd(ui, bar, false, Cmd::WebLink, "[ ]( )", mono, "Web link: [text](https://…)"));
                cmds.push(text_cmd(ui, bar, false, Cmd::Embed, "![[ ]]", mono, "Embed a file or picture: opens the file picker (![[file]])"));
                separator(ui, sc);
                cmds.push(draw_cmd(ui, bar, false, Cmd::Clear, "Clear formatting", draw_clear));
                out = cmds.into_iter().flatten().next();
            });
        });
        (resp.response.rect.size(), out)
    }

    /// One of the two colour buttons (0: highlight, 1: font colour) with its palette popup.
    fn palette(&mut self, ui: &mut Ui, which: usize, palettes: &Palettes, open_now: bool) -> Option<Cmd> {
        let mark = which == 0;
        let (tip, none_label) = if mark { ("Highlight colour", "No highlight") } else { ("Font colour", "Default colour") };
        let last = if mark { self.last_mark } else { self.last_font };
        let resp = tool(ui, palettes.scale, BUTTON + 10.0, false, true, tip, |p, r, c, s| {
            let g = Rect::from_center_size(r.center() + Vec2::new(-4.0, -1.0) * s, Vec2::splat(15.0 * s));
            if mark {
                p.rect_filled(g.shrink2(Vec2::new(0.0, s)), 3.0 * s, rgb(last));
                let t = text_galley(p, "A", Label { bold: true, ..Default::default() }, 12.5 * s, if colour_dark(last) { Color32::BLACK } else { Color32::WHITE });
                p.galley(g.center() - t.size() / 2.0, t, c);
            } else {
                let t = text_galley(p, "A", Label { bold: true, ..Default::default() }, 14.5 * s, c);
                p.galley(g.center() - t.size() / 2.0 - Vec2::new(0.0, 1.5 * s), t, c);
                p.rect_filled(Rect::from_min_size(Pos2::new(g.left() + s, g.bottom() - s), Vec2::new(g.width() - 2.0 * s, 3.0 * s)), s, rgb(last));
            }
            caret(p, Pos2::new(r.right() - 6.0 * s, r.center().y), c, s);
        });
        palettes.badges.paint(ui, resp.rect, if mark { Action::NoteHighlightColour } else { Action::NoteFontColour });
        if open_now {
            egui::Popup::toggle_id(ui.ctx(), egui::Popup::default_response_id(&resp));
        }
        // what the popup picked: a colour, or (`Some(None)`) "no colour"
        let mut pick: Option<Option<[u8; 3]>> = None;
        egui::Popup::from_toggle_button_response(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(190.0);
            ui.label(egui::RichText::new(tip).small().color(theme::MUTED));
            ui.horizontal_wrapped(|ui| {
                for c in if mark { palettes.highlight } else { palettes.font } {
                    let (r, sw) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                    ui.painter().rect_filled(r.shrink(1.0), 4.0, rgb(*c));
                    ui.painter().rect_stroke(r.shrink(1.0), 4.0, Stroke::new(1.0, if sw.hovered() { theme::TEXT_STRONG } else { theme::BORDER }), egui::StrokeKind::Inside);
                    let hex = crate::colour_tags::hex(*c);
                    let label = format!("{tip} {hex}");
                    crate::script::hint(&label, r);
                    sw.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label.clone()));
                    if sw.on_hover_text(hex).clicked() {
                        pick = Some(Some(*c));
                    }
                }
            });
            if ui.button(none_label).clicked() {
                pick = Some(None);
            }
            let open = &mut self.picker_open[which];
            if ui.button(if *open { "Custom colour ▴" } else { "Custom colour…" }).clicked() {
                *open = !*open;
            }
            if *open {
                let custom = if mark { &mut self.custom_mark } else { &mut self.custom_font };
                egui::color_picker::color_picker_color32(ui, custom, egui::color_picker::Alpha::Opaque);
                if ui.button("Apply").clicked() {
                    pick = Some(Some([custom.r(), custom.g(), custom.b()]));
                }
            }
            if pick.is_some() {
                ui.close();
            }
        });
        let colour = pick?;
        if let Some(c) = colour {
            *(if mark { &mut self.last_mark } else { &mut self.last_font }) = c;
        }
        Some(if mark { Cmd::HighlightColour(colour) } else { Cmd::FontColour(colour) })
    }
}

fn colour_dark(c: [u8; 3]) -> bool {
    crate::colour_tags::dark_text_on([c[0], c[1], c[2], 255])
}

/// The heading button: Normal text and H1 to H6 in a menu.
fn heading_menu(ui: &mut Ui, b: Look, text: &str, sel: &Range<usize>, open_now: bool) -> Option<Cmd> {
    let level = (0..=6u8).find(|n| *n > 0 && format::is_on(Cmd::Heading(*n), text, sel.clone())).unwrap_or(0);
    let (s, cell) = (b.scale, b.cell);
    let resp = tool(ui, s, BUTTON + 14.0, level > 0, !cell, "Heading level", |p, r, c, s| {
        let name = if level > 0 { format!("H{level}") } else { "H".to_string() };
        let t = text_galley(p, &name, Label { bold: true, ..Default::default() }, 14.0 * s, c);
        p.galley(r.center() - t.size() / 2.0 - Vec2::new(4.0 * s, 0.0), t, c);
        caret(p, Pos2::new(r.right() - 7.0 * s, r.center().y), c, s);
    });
    if !cell {
        b.badges.paint(ui, resp.rect, Action::NoteHeading);
        if open_now {
            egui::Popup::toggle_id(ui.ctx(), egui::Popup::default_response_id(&resp));
        }
    }
    let mut out = None;
    egui::Popup::from_toggle_button_response(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
        ui.set_min_width(130.0);
        for n in 0..=6u8 {
            let label = if n == 0 { "Normal text".to_string() } else { format!("Heading {n}") };
            let size = 13.0 + (crate::editor::heading_scale(n) - 1.0) * 6.0;
            let b = ui.add(egui::Button::new(egui::RichText::new(label.clone()).size(size).color(if n == level { theme::ACCENT } else { theme::TEXT })).frame(false).min_size(Vec2::new(120.0, 22.0)));
            crate::script::hint(&label, b.rect);
            b.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label.clone()));
            if b.clicked() {
                out = Some(Cmd::Heading(n));
            }
        }
    });
    out
}

/// How many preset colours a palette can have.
pub const MAX_PRESETS: usize = 8;

/// The settings row of a palette (EDIT-028): a colour picker per preset, "−" and "+" to take one
/// away or add one, and a button to go back to the defaults. True when something changed.
pub fn preset_editor(ui: &mut Ui, label: &str, colours: &mut Vec<[u8; 3]>, defaults: &[[u8; 3]]) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        let mut remove = None;
        for (i, c) in colours.iter_mut().enumerate() {
            changed |= egui::color_picker::color_edit_button_srgb(ui, c).on_hover_text(format!("{label} {}: {}", i + 1, crate::colour_tags::hex(*c))).changed();
            if ui.small_button("−").on_hover_text("Take this colour out").clicked() {
                remove = Some(i);
            }
        }
        if let Some(i) = remove {
            colours.remove(i);
            changed = true;
        }
        if colours.len() < MAX_PRESETS && ui.small_button("+").on_hover_text("Add a colour").clicked() {
            colours.push(colours.last().copied().unwrap_or([128, 128, 128]));
            changed = true;
        }
        if colours.as_slice() != defaults && ui.small_button("Defaults").on_hover_text("Back to the standard colours").clicked() {
            *colours = defaults.to_vec();
            changed = true;
        }
    });
    changed
}
