//! The widget of the keymap view (KEY-056, KEY-057): the layer and system controls, the keyboard with
//! its chips, the legend with its filters and the panel of the selected key.

use super::layout::{self, Cap};
use super::model::{chord_text, Category, Entry, Filter, Kind, Layer, Model, System, Where};
use crate::keys::{Action, Keymap};
use crate::theme;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};
use std::collections::BTreeMap;

/// The gap between two caps, as a share of the width of a key.
const GAP: f32 = 0.07;

/// What the view remembers between frames.
pub struct State {
    pub system: System,
    pub iso: bool,
    /// The layer the buttons chose (holding keys shows another one meanwhile).
    pub layer: Layer,
    /// The key the panel below the keyboard is about (none: nothing clicked yet).
    pub sel: Option<egui::Key>,
    pub filter: Filter,
    /// Where the controls were drawn (`system:linux`, `iso`, `layer:shift`, `mod:Ctrl`, `key:K`,
    /// `change:<id>`, `cat:<name>`, `scope:<name>`, `layer-of:<n>`), for tests.
    pub rects: Vec<(String, Rect)>,
    /// The model of the last frame and what it was made from.
    cache: Option<(System, BTreeMap<String, Vec<String>>, Model)>,
}

impl Default for State {
    fn default() -> State {
        State { system: System::running(), iso: false, layer: Layer::NONE, sel: None, filter: Filter::default(), rects: vec![], cache: None }
    }
}

impl State {
    /// The view a screenshot starts with: `keymap`, or `keymap:<system>:<layer>` such as `keymap:macos:cmd-shift`
    /// (KEY-059); anything it does not know stays as it is.
    pub fn from_view(view: &str) -> State {
        let mut parts = view.split(':').skip(1);
        let system = parts.next().and_then(System::from_id).unwrap_or_else(System::running);
        let layer = parts.next().and_then(|l| Layer::from_id(system, l)).unwrap_or(Layer::NONE);
        State { system, layer, ..State::default() }
    }

    /// The model for the keymap, made again when the keys or the system changed, and the keys it is made from.
    fn model(&mut self, keymap: &Keymap) -> (Model, BTreeMap<String, Vec<String>>) {
        let overrides = keymap.overrides();
        let model = match self.cache.take() {
            Some((s, o, m)) if s == self.system && o == overrides => m,
            _ => Model::new(keymap, self.system),
        };
        (model, overrides)
    }
}

/// The colours of the keyboard in a light or a dark theme.
struct Tokens {
    case: Color32,
    cap: Color32,
    cap_edge: Color32,
    cap_shadow: Color32,
    mod_cap: Color32,
    sys: Color32,
    text: Color32,
    muted: Color32,
    accent: Color32,
}

impl Tokens {
    fn get() -> Tokens {
        let p = theme::palette();
        if p.dark {
            Tokens {
                case: Color32::from_rgb(0x0d, 0x0f, 0x13),
                cap: Color32::from_rgb(0x2a, 0x2f, 0x37),
                cap_edge: Color32::from_rgb(0x35, 0x3b, 0x45),
                cap_shadow: Color32::from_rgb(0x0a, 0x0c, 0x0f),
                mod_cap: Color32::from_rgb(0x23, 0x27, 0x2e),
                sys: Color32::from_rgb(0xff, 0x7a, 0x70),
                text: p.text,
                muted: p.muted,
                accent: p.accent,
            }
        } else {
            Tokens {
                case: Color32::from_rgb(0xc9, 0xce, 0xd6),
                cap: Color32::from_rgb(0xfb, 0xfc, 0xfd),
                cap_edge: Color32::from_rgb(0xd9, 0xdd, 0xe3),
                cap_shadow: Color32::from_rgb(0xa9, 0xb0, 0xbb),
                mod_cap: Color32::from_rgb(0xe7, 0xea, 0xef),
                sys: Color32::from_rgb(0xc2, 0x41, 0x3b),
                text: p.text,
                muted: p.muted,
                accent: p.accent,
            }
        }
    }
}

fn colour_of(c: Category) -> Color32 {
    let [r, g, b] = c.colour(theme::palette().dark);
    Color32::from_rgb(r, g, b)
}

/// A choice among several: a selectable label whose place is kept for tests.
fn choice(ui: &mut Ui, rects: &mut Vec<(String, Rect)>, id: String, selected: bool, text: &str) -> bool {
    let r = ui.selectable_label(selected, text);
    rects.push((id, r.rect));
    r.clicked()
}

/// Draw the view. Returns the action whose key the user asked to change (the Change button of the panel).
pub fn show(ui: &mut Ui, st: &mut State, keymap: &Keymap) -> Option<Action> {
    st.rects.clear();
    let (model, overrides) = st.model(keymap);
    let t = Tokens::get();
    // holding modifiers shows their layer meanwhile (KEY-056)
    let held = ui.input(|i| i.modifiers);
    let peek = Layer { ctrl: held.command, alt: held.alt, shift: held.shift, sup: keymap.is_mac() && held.ctrl };
    let layer = if peek.is_none() { st.layer } else { peek };

    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("System").color(t.muted));
        for s in System::ALL {
            if choice(ui, &mut st.rects, format!("system:{}", s.id()), st.system == s, s.label()) {
                st.system = s;
            }
        }
        ui.add_space(10.0);
        ui.label(RichText::new("Keyboard").color(t.muted));
        for (iso, name) in [(false, "ANSI (US)"), (true, "ISO")] {
            if choice(ui, &mut st.rects, format!("keyboard:{}", if iso { "iso" } else { "ansi" }), st.iso == iso, name) {
                st.iso = iso;
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Layer").color(t.muted));
        for l in Layer::PRESETS {
            let name = if l.is_none() { "None".to_string() } else { l.name(st.system) };
            if choice(ui, &mut st.rects, format!("layer:{}", l.id(System::Linux)), layer == l, &name) {
                st.layer = l;
            }
        }
        ui.label(RichText::new(format!("showing {}{}", layer.name(st.system), if peek.is_none() { "" } else { " (keys held)" })).color(t.muted));
    });
    // the legend: the kinds of action, with how many are on this layer and in all
    let keys_here = layout::keys_on(st.iso, st.system);
    ui.horizontal_wrapped(|ui| {
        for cat in Category::ALL {
            let total = model.entries().iter().filter(|e| e.cat == cat && !st.filter.hidden_scopes.contains(&e.scope)).count();
            let here = model.entries().iter().filter(|e| e.cat == cat && e.layer == layer && keys_here.contains(&e.key) && !st.filter.hidden_scopes.contains(&e.scope)).count();
            let hidden = st.filter.hidden_cats.contains(&cat);
            let r = ui.add(egui::Button::new(format!("      {}  {here} / {total}", cat.name())).selected(!hidden));
            ui.painter().rect_filled(Rect::from_center_size(Pos2::new(r.rect.left() + 13.0, r.rect.center().y), Vec2::splat(10.0)), CornerRadius::same(3), if hidden { colour_of(cat).gamma_multiply(0.4) } else { colour_of(cat) });
            st.rects.push((format!("cat:{}", cat.name()), r.rect));
            if r.on_hover_text("Show or hide this kind of action on the keys").clicked() {
                toggle(&mut st.filter.hidden_cats, cat);
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Where").color(t.muted));
        for w in Where::ALL.into_iter().filter(|w| model.entries().iter().any(|e| e.scope == *w)) {
            let n = model.entries().iter().filter(|e| e.scope == w).count();
            let r = ui.selectable_label(!st.filter.hidden_scopes.contains(&w), format!("{} {n}", w.label()));
            st.rects.push((format!("scope:{}", w.label()), r.rect));
            if r.clicked() {
                toggle(&mut st.filter.hidden_scopes, w);
            }
        }
        if ui.small_button("Show all").clicked() {
            st.filter = Filter::default();
        }
    });
    ui.add_space(6.0);

    board(ui, st, &model, layer, &t);

    ui.add_space(8.0);
    let change = detail(ui, st, &model, layer, &t);
    let sequences = model.sequences();
    if !sequences.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new("Sequences (press one key after the other)").strong());
        ui.horizontal_wrapped(|ui| {
            for e in sequences.into_iter().filter(|e| st.filter.shows(e)) {
                ui.label(RichText::new(format!("{}  {}", e.text, e.short)).color(colour_of(e.cat)));
            }
        });
    }
    st.cache = Some((model.system, overrides, model));
    change
}

fn toggle<T: PartialEq>(list: &mut Vec<T>, item: T) {
    match list.iter().position(|x| *x == item) {
        Some(i) => {
            list.remove(i);
        }
        None => list.push(item),
    }
}

/// The keyboard.
fn board(ui: &mut Ui, st: &mut State, model: &Model, layer: Layer, t: &Tokens) {
    let rows = layout::main_rows(st.iso, st.system);
    let nav = layout::nav_rows();
    let pad = 0.28;
    let main_w = layout::row_width(&rows[1], GAP);
    let nav_w = layout::row_width(&nav[1], GAP);
    let total_units = main_w + 0.5 + nav_w + 2.0 * pad;
    let avail = ui.available_width().max(300.0);
    let u = (avail / total_units).clamp(20.0, 70.0);
    let g = GAP * u;
    let row_h = 1.02 * u;
    let total_h = rows.len() as f32 * row_h + (rows.len() - 1) as f32 * g + 0.22 * u + 2.0 * pad * u;
    let (outer, _) = ui.allocate_exact_size(Vec2::new(total_units * u, total_h), Sense::hover());
    let painter = ui.painter_at(outer);
    painter.rect_filled(outer, CornerRadius::same((0.3 * u) as u8), t.case);
    let paint = Paint { p: &painter, u, t };
    let frame = Frame { model, layer };
    let origin = outer.min + Vec2::splat(pad * u);
    let mut toggled = None;
    let mut picked = None;
    for (block, (x0, grid)) in [(origin.x, &rows), (origin.x + (main_w + 0.5) * u, &nav)].into_iter().enumerate() {
        for (i, row) in grid.iter().enumerate() {
            let mut y = origin.y + i as f32 * (row_h + g);
            if i > 0 {
                y += 0.22 * u;
            }
            let mut x = x0;
            for cap in row {
                let w = (cap.width() * u + (cap.width() - 1.0) * g).max(0.0);
                let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, row_h));
                x += w + g;
                let tag = format!("{block}-{i}-{}", rect.min.x as i32);
                match *cap {
                    Cap::Gap(_) => {}
                    Cap::Dead { legend, .. } => paint.cap(rect, legend, Look { fill: t.mod_cap, alpha: 0.55, ..Look::KEY }),
                    Cap::Mod { m, legend, .. } => {
                        let on = layer.has(m);
                        paint.cap(rect, legend, Look { on, modifier: true, fill: t.mod_cap, alpha: 1.0 });
                        let r = ui.interact(rect, ui.make_persistent_id(("mod", tag)), Sense::click());
                        st.rects.push((format!("mod:{m:?}"), rect));
                        if r.on_hover_text(format!("Hold {} in the layer", st.system.name_of(m))).clicked() {
                            toggled = Some(m);
                        }
                    }
                    Cap::Key { key, legend, .. } => {
                        if paint.key_cap(ui, st, &frame, &KeyCap { rect, key, legend, chips: true, tag: &tag }) {
                            picked = Some(key);
                        }
                    }
                    Cap::Cont { key, .. } => {
                        if paint.key_cap(ui, st, &frame, &KeyCap { rect, key, legend: "", chips: false, tag: &tag }) {
                            picked = Some(key);
                        }
                    }
                }
            }
        }
    }
    if let Some(m) = toggled {
        st.layer = st.layer.with(m, !st.layer.has(m));
    }
    if let Some(k) = picked {
        st.sel = Some(k);
    }
}

/// What the caps are painted with: the painter, the size of a key and the colours.
struct Paint<'a> {
    p: &'a egui::Painter,
    u: f32,
    t: &'a Tokens,
}

/// The state of the keyboard a cap is drawn for.
struct Frame<'a> {
    model: &'a Model,
    layer: Layer,
}

/// A key cap to draw.
struct KeyCap<'a> {
    rect: Rect,
    key: egui::Key,
    legend: &'a str,
    /// Draw the chips and keep the place for the tests (not the lower part of an ISO Enter).
    chips: bool,
    /// What tells this cap from the others of the same key.
    tag: &'a str,
}

/// How a cap looks: `on` fills it with the accent, `modifier` is the look of a modifier key.
struct Look {
    on: bool,
    modifier: bool,
    fill: Color32,
    alpha: f32,
}

impl Look {
    /// An ordinary key; the fill is set by the caller.
    const KEY: Look = Look { on: false, modifier: false, fill: Color32::TRANSPARENT, alpha: 1.0 };
}

impl Paint<'_> {
    /// One cap with its legend.
    fn cap(&self, rect: Rect, legend: &str, look: Look) {
        let (p, u, t) = (self.p, self.u, self.t);
        let fill = if look.on { t.accent } else { look.fill };
        let r = CornerRadius::same((0.1 * u) as u8);
        p.rect_filled(rect.translate(Vec2::new(0.0, 0.03 * u)), r, t.cap_shadow.gamma_multiply(look.alpha));
        p.rect_filled(rect, r, fill.gamma_multiply(look.alpha));
        p.rect_stroke(rect, r, Stroke::new(1.0, t.cap_edge), StrokeKind::Inside);
        let size = (if look.modifier { 0.23 } else { 0.25 } * u).max(8.0);
        if !legend.is_empty() {
            p.text(rect.min + Vec2::new(0.1 * u, 0.06 * u), Align2::LEFT_TOP, legend, FontId::proportional(size), if look.on { Color32::WHITE } else { t.muted.gamma_multiply(look.alpha) });
        }
    }

    /// The cap of a key that can have actions: legend, hatch when the system keeps it, chips. True when it was clicked.
    fn key_cap(&self, ui: &mut Ui, st: &mut State, frame: &Frame, cap: &KeyCap) -> bool {
        let (p, u, t) = (self.p, self.u, self.t);
        let (rect, key) = (cap.rect, cap.key);
        let reserved = frame.model.reserved(frame.layer, key);
        let all = frame.model.at(frame.layer, key);
        let shown: Vec<&Entry> = all.iter().copied().filter(|e| st.filter.shows(e)).collect();
        self.cap(rect, cap.legend, Look { fill: t.cap, ..Look::KEY });
        if reserved.is_some() {
            hatch(p, rect, u, t.sys);
        }
        if cap.chips {
            let font = FontId::proportional((0.2 * u).max(7.0));
            let chip_h = 0.27 * u;
            for (i, e) in shown.iter().take(2).enumerate() {
                let top = rect.top() + 0.34 * u + i as f32 * (chip_h + 0.03 * u);
                let chip = Rect::from_min_size(Pos2::new(rect.left() + 0.06 * u, top), Vec2::new(rect.width() - 0.12 * u, chip_h));
                let cc = colour_of(e.cat);
                let fill = t.cap.lerp_to_gamma(cc, if e.kind == Kind::Fixed { 0.1 } else { 0.22 });
                p.rect_filled(chip, CornerRadius::same(2), fill);
                p.rect_filled(Rect::from_min_size(chip.min, Vec2::new(3.0, chip.height())), CornerRadius::same(1), cc);
                if e.kind == Kind::Changed {
                    p.rect_stroke(chip, CornerRadius::same(2), Stroke::new(1.0, t.text), StrokeKind::Inside);
                }
                let text = if e.sequence { format!("» {}", e.short) } else { e.short.clone() };
                p.with_clip_rect(chip.shrink2(Vec2::new(1.0, 0.0))).text(Pos2::new(chip.left() + 6.0, chip.center().y), Align2::LEFT_CENTER, text, font.clone(), t.text);
            }
            if shown.len() > 2 {
                p.text(Pos2::new(rect.right() - 0.06 * u, rect.bottom() - 0.04 * u), Align2::RIGHT_BOTTOM, format!("+{}", shown.len() - 2), FontId::monospace((0.17 * u).max(7.0)), t.muted);
            }
            if let (Some(label), true) = (reserved, shown.is_empty()) {
                p.with_clip_rect(rect.shrink(2.0)).text(Pos2::new(rect.left() + 0.08 * u, rect.bottom() - 0.05 * u), Align2::LEFT_BOTTOM, label, FontId::proportional((0.19 * u).max(7.0)), t.sys);
            }
        }
        if st.sel == Some(key) {
            p.rect_stroke(rect.expand(1.0), CornerRadius::same((0.1 * u) as u8), Stroke::new(2.5, t.text), StrokeKind::Outside);
        }
        let r = ui.interact(rect, ui.make_persistent_id(("cap", cap.tag)), Sense::click());
        if cap.chips {
            st.rects.push((format!("key:{}", key.name()), rect));
        }
        let mut tip = vec![chord_text(st.system, frame.layer, key)];
        tip.extend(all.iter().map(|e| e.label.to_string()));
        tip.extend(reserved.map(|r| format!("System: {r}")));
        r.on_hover_text(tip.join("\n")).clicked()
    }
}

/// Diagonal stripes in the colour of the system, for a key it keeps for itself.
fn hatch(p: &egui::Painter, rect: Rect, u: f32, colour: Color32) {
    let clipped = p.with_clip_rect(rect);
    let step = (0.15 * u).max(5.0);
    let stroke = Stroke::new(1.0, colour.gamma_multiply(0.45));
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        clipped.line_segment([Pos2::new(x, rect.bottom()), Pos2::new(x + rect.height(), rect.top())], stroke);
        x += step;
    }
    p.rect_stroke(rect, CornerRadius::same((0.1 * u) as u8), Stroke::new(1.0, colour), StrokeKind::Inside);
}

/// The panel of the selected key: what is on it on this layer, and on every layer.
fn detail(ui: &mut Ui, st: &mut State, model: &Model, layer: Layer, t: &Tokens) -> Option<Action> {
    let mut change = None;
    let Some(key) = st.sel else {
        ui.label(RichText::new("Click a key to see what is on it, on this layer and on every other.").color(t.muted));
        return None;
    };
    ui.label(RichText::new(chord_text(st.system, layer, key)).monospace().size(18.0).strong());
    let on = model.at(layer, key);
    if let Some(label) = model.reserved(layer, key) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).color(t.sys));
            ui.label(RichText::new("system").small().color(t.sys));
        });
        ui.label(RichText::new("The system or the desktop takes this key; Shale must not bind it.").small().color(t.muted));
    }
    if on.is_empty() && model.reserved(layer, key).is_none() {
        ui.label(RichText::new("Free: nothing in Shale uses this key on this layer.").color(t.muted));
    }
    for e in on {
        ui.horizontal_wrapped(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::same(3), colour_of(e.cat));
            ui.label(e.label);
            let kind = match e.kind {
                Kind::Default => "default",
                Kind::Changed => "your key",
                Kind::Fixed => "built in",
            };
            ui.label(RichText::new(kind).small().color(if e.kind == Kind::Changed { t.accent } else { t.muted }));
            if let Some(a) = e.action {
                let r = ui.small_button("Change");
                st.rects.push((format!("change:{}", a.def().id), r.rect));
                if r.on_hover_text("Go to the row of this action in Settings, Keybindings, and press the new keys").clicked() {
                    change = Some(a);
                }
            }
        });
        let note = if e.note.is_empty() { String::new() } else { format!(" · {}", e.note) };
        ui.label(RichText::new(format!("{} · {} · {}{note}", e.cat.name(), e.scope.label(), e.text)).small().color(t.muted));
    }
    ui.add_space(4.0);
    ui.label(RichText::new("This key on every layer").strong());
    for (n, ol) in model.layers_of(key).into_iter().enumerate() {
        let mut what: Vec<String> = ol.entries.iter().map(|e| e.short.clone()).collect();
        what.extend(ol.reserved.map(|r| format!("System: {r}")));
        what.dedup();
        let r = ui.add(egui::Button::new(format!("{}   {}", chord_text(st.system, ol.layer, key), what.join(" · "))).frame(false).selected(ol.layer == layer));
        st.rects.push((format!("layer-of:{n}"), r.rect));
        if r.clicked() {
            st.layer = ol.layer;
        }
    }
    change
}
