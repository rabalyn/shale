//! Fonts and text sizes (CFG-030 to CFG-037): one font and size for each area of the window,
//! the system fonts to choose from, and the egui fonts and text styles that follow from the
//! settings.
//!
//! The settings are `Config::fonts` ([`FontsCfg`]). Every frame [`FontState::sync`] compares them
//! with what is in use and, when they differ, loads the chosen system fonts into egui, so a change
//! of the settings (in the dialog or in the settings file) is used at once and fonts are only
//! rebuilt when the set of chosen families changes.

use crate::theme;
use eframe::egui::{self, FontFamily, FontId, Ui};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// The text size of the dialogs, which the sizes of the text styles are in proportion to.
pub const UI_SIZE: f32 = 14.0;

/// The smallest and largest text size that is used, whatever the settings file says.
pub const MIN_SIZE: f32 = 8.0;
pub const MAX_SIZE: f32 = 48.0;

/// A part of the window that has a font and size of its own (CFG-030).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Area {
    /// Dialogs, menus, sidebars and everything that is not listed below.
    Ui,
    /// The raw markdown text of the editor (also the raw lines around the cursor in the preview).
    EditorRaw,
    /// The rendered text: the lines of the markdown preview and the reading view.
    EditorRendered,
    Mail,
    Calendar,
    Social,
}

impl Area {
    pub const ALL: [Area; 6] = [Area::Ui, Area::EditorRaw, Area::EditorRendered, Area::Mail, Area::Calendar, Area::Social];

    pub fn label(self) -> &'static str {
        match self {
            Area::Ui => "Dialogs and menus",
            Area::EditorRaw => "Editor: raw markdown",
            Area::EditorRendered => "Editor: rendered text and reading view",
            Area::Mail => "Mail",
            Area::Calendar => "Calendar",
            Area::Social => "Feeds",
        }
    }

    /// The size this area has until it is changed.
    pub fn default_size(self) -> f32 {
        match self {
            Area::EditorRaw | Area::EditorRendered => 15.5,
            _ => UI_SIZE,
        }
    }
}

/// The font of an area as it is written in the settings: `None` is Shale's own font.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct FontChoice {
    pub family: Option<String>,
    pub size: f32,
}

impl FontChoice {
    fn new(size: f32) -> FontChoice {
        FontChoice { family: None, size }
    }
}

/// The font settings (CFG-030, CFG-031): one font for everything, or one for each area.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(from = "FontsRaw")]
pub struct FontsCfg {
    /// true: `all` is used for every area; the choices of the single areas rest.
    pub same_everywhere: bool,
    pub all: FontChoice,
    pub ui: FontChoice,
    pub editor_raw: FontChoice,
    pub editor_rendered: FontChoice,
    pub mail: FontChoice,
    pub calendar: FontChoice,
    pub social: FontChoice,
}

impl Default for FontsCfg {
    fn default() -> Self {
        FontsCfg {
            same_everywhere: true,
            all: FontChoice::new(UI_SIZE),
            ui: FontChoice::new(Area::Ui.default_size()),
            editor_raw: FontChoice::new(Area::EditorRaw.default_size()),
            editor_rendered: FontChoice::new(Area::EditorRendered.default_size()),
            mail: FontChoice::new(Area::Mail.default_size()),
            calendar: FontChoice::new(Area::Calendar.default_size()),
            social: FontChoice::new(Area::Social.default_size()),
        }
    }
}

/// What a settings file may hold of one area: a part that is missing takes the default of the area.
#[derive(Deserialize, Default)]
#[serde(default)]
struct ChoiceRaw {
    family: Option<String>,
    size: Option<f32>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct FontsRaw {
    same_everywhere: Option<bool>,
    all: Option<ChoiceRaw>,
    ui: Option<ChoiceRaw>,
    editor_raw: Option<ChoiceRaw>,
    editor_rendered: Option<ChoiceRaw>,
    mail: Option<ChoiceRaw>,
    calendar: Option<ChoiceRaw>,
    social: Option<ChoiceRaw>,
}

impl From<FontsRaw> for FontsCfg {
    fn from(raw: FontsRaw) -> FontsCfg {
        let d = FontsCfg::default();
        let pick = |r: Option<ChoiceRaw>, d: FontChoice| match r {
            Some(c) => FontChoice { family: c.family, size: c.size.unwrap_or(d.size) },
            None => d,
        };
        FontsCfg {
            same_everywhere: raw.same_everywhere.unwrap_or(d.same_everywhere),
            all: pick(raw.all, d.all),
            ui: pick(raw.ui, d.ui),
            editor_raw: pick(raw.editor_raw, d.editor_raw),
            editor_rendered: pick(raw.editor_rendered, d.editor_rendered),
            mail: pick(raw.mail, d.mail),
            calendar: pick(raw.calendar, d.calendar),
            social: pick(raw.social, d.social),
        }
    }
}

impl FontsCfg {
    /// The font and size an area has now (CFG-030, CFG-031). Where everything shares one font
    /// the size is the dialogs' size, and the other areas keep the proportion their defaults
    /// have to it (the editor's text is a little larger than a dialog's), so the defaults
    /// look as they always did.
    pub fn choice(&self, area: Area) -> FontChoice {
        let (family, size) = if self.same_everywhere {
            (self.all.family.clone(), self.all.size * area.default_size() / UI_SIZE)
        } else {
            let c = self.slot(area);
            (c.family.clone(), c.size)
        };
        let size = if size.is_finite() { size.clamp(MIN_SIZE, MAX_SIZE) } else { area.default_size() };
        FontChoice { family: family.filter(|f| !f.trim().is_empty()), size }
    }

    fn slot(&self, area: Area) -> &FontChoice {
        match area {
            Area::Ui => &self.ui,
            Area::EditorRaw => &self.editor_raw,
            Area::EditorRendered => &self.editor_rendered,
            Area::Mail => &self.mail,
            Area::Calendar => &self.calendar,
            Area::Social => &self.social,
        }
    }

    pub fn slot_mut(&mut self, area: Area) -> &mut FontChoice {
        match area {
            Area::Ui => &mut self.ui,
            Area::EditorRaw => &mut self.editor_raw,
            Area::EditorRendered => &mut self.editor_rendered,
            Area::Mail => &mut self.mail,
            Area::Calendar => &mut self.calendar,
            Area::Social => &mut self.social,
        }
    }

    /// Stop sharing one font: every area starts from what it has now (CFG-031).
    pub fn separate(&mut self) {
        for a in Area::ALL {
            *self.slot_mut(a) = self.choice(a);
        }
        self.same_everywhere = false;
    }

    /// The families the settings ask for.
    fn families(&self) -> BTreeSet<String> {
        Area::ALL.iter().filter_map(|a| self.choice(*a).family).collect()
    }
}

/// A font that can be used in egui: the family and the size in points.
#[derive(Clone, Debug, PartialEq)]
pub struct AreaFont {
    pub family: FontFamily,
    pub size: f32,
}

/// The two fonts of the editor (CFG-032).
#[derive(Clone, Debug, PartialEq)]
pub struct EditorFonts {
    pub raw: AreaFont,
    pub rendered: AreaFont,
}

impl EditorFonts {
    /// One size in Shale's own font for both (tests).
    #[cfg(test)]
    pub fn uniform(size: f32) -> EditorFonts {
        let f = AreaFont { family: FontFamily::Proportional, size };
        EditorFonts { raw: f.clone(), rendered: f }
    }
}

/// The name of the egui family that holds the system font `name`.
fn family_of(name: &str) -> FontFamily {
    FontFamily::Name(format!("system:{name}").into())
}

/// The font of every area as egui uses it.
#[derive(Clone, Debug, PartialEq)]
struct Resolved([AreaFont; 6]);

impl Default for Resolved {
    fn default() -> Self {
        Resolved(Area::ALL.map(|a| AreaFont { family: FontFamily::Proportional, size: a.default_size() }))
    }
}

impl Resolved {
    fn get(&self, area: Area) -> &AreaFont {
        &self.0[Area::ALL.iter().position(|a| *a == area).unwrap_or(0)]
    }
}

/// The installed fonts: found by a thread that reads the font folders, or handed over ready.
#[derive(Default)]
pub struct SystemFonts {
    rx: Option<mpsc::Receiver<fontdb::Database>>,
    db: Option<Arc<fontdb::Database>>,
    /// The family names, each once, sorted.
    families: Vec<String>,
    started: bool,
}

impl SystemFonts {
    /// Read the font folders of the system in the background, once.
    pub fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let spawned = std::thread::Builder::new().name("system-fonts".into()).spawn(move || {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            let _ = tx.send(db);
        });
        if spawned.is_err() {
            self.set(fontdb::Database::new());
        }
    }

    /// Take the fonts of `db` as the installed ones.
    pub fn set(&mut self, db: fontdb::Database) {
        let mut names: Vec<String> = db.faces().filter_map(|f| f.families.first().map(|(n, _)| n.clone())).collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        self.families = names;
        self.db = Some(Arc::new(db));
        self.started = true;
        self.rx = None;
    }

    /// Pick up the result of the background search, if it is done.
    pub fn poll(&mut self) {
        if let Some(Ok(db)) = self.rx.as_ref().map(|rx| rx.try_recv()) {
            self.set(db);
        }
    }

    pub fn ready(&self) -> bool {
        self.db.is_some()
    }

    pub fn families(&self) -> &[String] {
        &self.families
    }

    /// The regular face of a family: the bytes of its file and the index of the face in it.
    fn face(&self, name: &str) -> Option<(Vec<u8>, u32)> {
        let db = self.db.as_ref()?;
        let q = fontdb::Query { families: &[fontdb::Family::Name(name)], weight: fontdb::Weight::NORMAL, stretch: fontdb::Stretch::Normal, style: fontdb::Style::Normal };
        let id = db.query(&q)?;
        db.with_face_data(id, |data, index| (data.to_vec(), index))
    }
}

/// What is in use, and what the settings ask for (CFG-036).
pub struct FontState {
    pub system: SystemFonts,
    resolved: Resolved,
    /// Fonts handed to egui in this pass are used from the next one on.
    pending: Option<(Resolved, u64)>,
    /// The settings `resolved` (or `pending`) was made from.
    seen: Option<FontsCfg>,
    /// A chosen family waits for the search for the installed fonts to end.
    waiting: bool,
    /// The system families that are loaded in egui.
    installed: BTreeSet<String>,
    /// The families that were said to be missing.
    told: BTreeSet<String>,
}

impl FontState {
    pub fn new() -> FontState {
        FontState { system: SystemFonts::default(), resolved: Resolved::default(), pending: None, seen: None, waiting: false, installed: BTreeSet::new(), told: BTreeSet::new() }
    }

    /// The font an area is drawn in.
    pub fn area(&self, area: Area) -> AreaFont {
        self.resolved.get(area).clone()
    }

    pub fn editor(&self) -> EditorFonts {
        EditorFonts { raw: self.area(Area::EditorRaw), rendered: self.area(Area::EditorRendered) }
    }

    /// Make what is in use follow the settings. Comes back with something to tell the user when
    /// a chosen font is not installed (CFG-035).
    pub fn sync(&mut self, ctx: &egui::Context, cfg: &FontsCfg) -> Option<String> {
        if let Some((r, pass)) = self.pending.take() {
            if ctx.cumulative_pass_nr() > pass {
                self.use_resolved(ctx, r);
            } else {
                self.pending = Some((r, pass));
            }
        }
        let was_ready = self.system.ready();
        self.system.poll();
        let arrived = self.system.ready() && !was_ready;
        let wanted = cfg.families();
        if self.seen.as_ref() == Some(cfg) && !(self.waiting && arrived) {
            if self.waiting {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            return None;
        }
        self.seen = Some(cfg.clone());
        if !wanted.is_empty() {
            self.system.start();
        }
        self.waiting = !wanted.is_empty() && !self.system.ready();
        let mut resolved = Resolved::default();
        let mut missing = BTreeSet::new();
        let mut loaded: Vec<(String, egui::FontData)> = Vec::new();
        for (i, area) in Area::ALL.iter().enumerate() {
            let c = cfg.choice(*area);
            resolved.0[i].size = c.size;
            let Some(name) = c.family else { continue };
            if !self.system.ready() {
                continue;
            }
            if loaded.iter().any(|(n, _)| *n == name) {
                resolved.0[i].family = family_of(&name);
            } else if let Some((bytes, index)) = self.system.face(&name) {
                let mut data = egui::FontData::from_owned(bytes);
                data.index = index;
                loaded.push((name.clone(), data));
                resolved.0[i].family = family_of(&name);
            } else {
                missing.insert(name);
            }
        }
        let now: BTreeSet<String> = loaded.iter().map(|(n, _)| n.clone()).collect();
        if self.waiting {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if now != self.installed {
            ctx.set_fonts(theme::font_definitions(loaded.into_iter().map(|(n, d)| (family_of(&n), n, d)).collect()));
            self.installed = now;
            self.pending = Some((resolved, ctx.cumulative_pass_nr()));
        } else {
            self.use_resolved(ctx, resolved);
        }
        let fresh: Vec<String> = missing.difference(&self.told).cloned().collect();
        self.told = missing;
        if fresh.is_empty() {
            None
        } else {
            Some(format!("The font {} is not installed, so Shale's own font is used instead", fresh.iter().map(|f| format!("\"{f}\"")).collect::<Vec<_>>().join(", ")))
        }
    }

    fn use_resolved(&mut self, ctx: &egui::Context, r: Resolved) {
        if r.get(Area::Ui) != self.resolved.get(Area::Ui) {
            theme::set_text_styles(ctx, r.get(Area::Ui));
        }
        self.resolved = r;
    }
}

thread_local! {
    /// The font of the area that is being drawn (set by `with_area`).
    static ACTIVE: std::cell::RefCell<Option<AreaFont>> = const { std::cell::RefCell::new(None) };
}

/// Draw `add` in the font of an area (CFG-033): the text styles of its `ui` are those of
/// `font`, so every label and button in it follows, and `px` and `font` give the hand-sized
/// text the same size and family.
pub fn with_area<R>(ui: &mut Ui, font: &AreaFont, add: impl FnOnce(&mut Ui) -> R) -> R {
    let before = ACTIVE.with(|a| a.replace(Some(font.clone())));
    let r = ui
        .scope(|ui| {
            ui.style_mut().text_styles = theme::text_styles(font);
            add(ui)
        })
        .inner;
    ACTIVE.with(|a| *a.borrow_mut() = before);
    r
}

/// Like `with_area`, for code that draws on its own and only needs `family` (the editor).
pub fn with_family<R>(font: &AreaFont, add: impl FnOnce() -> R) -> R {
    let before = ACTIVE.with(|a| a.replace(Some(font.clone())));
    let r = add();
    ACTIVE.with(|a| *a.borrow_mut() = before);
    r
}

/// The family of the area that is being drawn: Shale's own font outside of an area.
pub fn family() -> FontFamily {
    ACTIVE.with(|a| a.borrow().as_ref().map_or(FontFamily::Proportional, |f| f.family.clone()))
}

/// A size of the 14-point design of the dialogs, in the size of the area that is drawn.
pub fn px(size: f32) -> f32 {
    ACTIVE.with(|a| a.borrow().as_ref().map_or(size, |f| size * f.size / UI_SIZE))
}

/// A font of `size` points of the 14-point design, in the font and size of the area that is drawn.
pub fn font(size: f32) -> FontId {
    ACTIVE.with(|a| a.borrow().as_ref().map_or_else(|| FontId::proportional(size), |f| FontId::new(size * f.size / UI_SIZE, f.family.clone())))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::theme;

    // spec: CFG-030
    #[test]
    fn cfg_030_every_area_has_a_font_and_the_defaults_keep_the_old_sizes() {
        let c = FontsCfg::default();
        assert!(c.same_everywhere);
        let sizes: Vec<f32> = Area::ALL.iter().map(|a| c.choice(*a).size).collect();
        assert_eq!(sizes, [14.0, 15.5, 15.5, 14.0, 14.0, 14.0]);
        assert!(Area::ALL.iter().all(|a| c.choice(*a).family.is_none()));
    }

    // spec: CFG-031
    #[test]
    fn cfg_031_one_font_for_everything_or_one_for_each_area() {
        let mut c = FontsCfg { all: FontChoice { family: Some("Serif".into()), size: 28.0 }, ..FontsCfg::default() };
        for a in Area::ALL {
            let ch = c.choice(a);
            assert_eq!(ch.family.as_deref(), Some("Serif"));
            assert_eq!(ch.size, 28.0 * a.default_size() / 14.0);
        }
        // switching it off starts every area from what it showed
        c.separate();
        assert!(!c.same_everywhere);
        assert_eq!(c.editor_raw, FontChoice { family: Some("Serif".into()), size: 28.0 * 15.5 / 14.0 });
        c.editor_raw.size = 12.0;
        assert_eq!((c.choice(Area::EditorRaw).size, c.choice(Area::EditorRendered).size), (12.0, 28.0 * 15.5 / 14.0));
        // the single choices rest, they are not lost, while everything is shared again
        c.same_everywhere = true;
        assert_eq!(c.choice(Area::EditorRaw).size, 28.0 * 15.5 / 14.0);
        assert_eq!(c.editor_raw.size, 12.0);
    }

    // spec: CFG-030
    #[test]
    fn cfg_030_a_size_outside_the_range_or_not_a_number_is_corrected() {
        let with = |family: Option<&str>, size: f32| FontsCfg { all: FontChoice { family: family.map(str::to_string), size }, ..FontsCfg::default() };
        assert_eq!(with(None, 900.0).choice(Area::Ui).size, MAX_SIZE);
        assert_eq!(with(None, f32::NAN).choice(Area::Ui).size, 14.0);
        let c = with(Some("  "), 1.0).choice(Area::Ui);
        assert_eq!((c.size, c.family), (MIN_SIZE, None));
    }

    // spec: CFG-030
    #[test]
    fn cfg_030_a_part_of_an_area_that_is_left_out_takes_its_default() {
        let c: FontsCfg = serde_json::from_str(r#"{"same_everywhere": false, "editor_raw": {"family": "Mono"}, "mail": {"size": 18.0}}"#).unwrap();
        assert_eq!(c.editor_raw, FontChoice { family: Some("Mono".into()), size: 15.5 });
        assert_eq!((c.mail.size, c.calendar.size, c.ui.size), (18.0, 14.0, 14.0));
        assert!(!c.same_everywhere);
    }

    /// A database that holds only the bundled Virgil font, and the name it has there: the
    /// "installed fonts" of the tests, whatever is on the machine that runs them.
    pub(crate) fn virgil_db() -> (fontdb::Database, String) {
        let mut db = fontdb::Database::new();
        db.load_font_data(include_bytes!("../assets/fonts/Virgil.otf").to_vec());
        let name = db.faces().next().and_then(|f| f.families.first().map(|(n, _)| n.clone())).expect("Virgil has a family name");
        (db, name)
    }

    fn pass(ctx: &egui::Context) {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            ui.label("text");
        });
        out.textures_delta.clear();
    }

    // spec: CFG-032
    #[test]
    fn cfg_032_the_installed_fonts_are_listed_once_each_and_a_face_can_be_loaded() {
        let (db, name) = virgil_db();
        let mut s = SystemFonts::default();
        assert!(!s.ready() && s.families().is_empty());
        s.set(db);
        assert!(s.ready());
        assert_eq!(s.families(), std::slice::from_ref(&name));
        let (bytes, index) = s.face(&name).expect("the face loads");
        assert!(bytes.len() > 1000 && index == 0);
        assert!(s.face("No Such Family").is_none());
    }

    // spec: CFG-035
    #[test]
    fn cfg_035_fonts_are_rebuilt_only_when_the_families_change_and_a_missing_font_is_told_once() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let (db, name) = virgil_db();
        let mut st = FontState::new();
        st.system.set(db);
        let mut cfg = FontsCfg::default();
        assert_eq!(st.sync(&ctx, &cfg), None);
        // only a size: no fonts are loaded
        cfg.all.size = 20.0;
        assert_eq!(st.sync(&ctx, &cfg), None);
        assert!(st.installed.is_empty() && st.pending.is_none());
        assert_eq!(st.area(Area::Ui).size, 20.0);
        assert_eq!(ctx.global_style().text_styles[&egui::TextStyle::Body].size, 20.0);
        // a family: the font is loaded, and used from the next pass on
        cfg.all.family = Some(name.clone());
        assert_eq!(st.sync(&ctx, &cfg), None);
        assert_eq!(st.installed, BTreeSet::from([name.clone()]));
        assert_eq!(st.area(Area::Ui).family, FontFamily::Proportional, "not yet");
        pass(&ctx);
        assert_eq!(st.sync(&ctx, &cfg), None);
        assert_eq!(st.area(Area::Ui).family, family_of(&name));
        assert_eq!(st.area(Area::EditorRaw).family, family_of(&name));
        assert_eq!(ctx.global_style().text_styles[&egui::TextStyle::Body].family, family_of(&name));
        assert_eq!(ctx.global_style().text_styles[&egui::TextStyle::Monospace].family, FontFamily::Monospace, "code stays monospace");
        pass(&ctx); // text can be laid out in the family
                    // a size again: nothing is rebuilt
        cfg.all.size = 22.0;
        assert_eq!(st.sync(&ctx, &cfg), None);
        assert!(st.pending.is_none() && st.area(Area::Ui).size == 22.0 && st.area(Area::Ui).family == family_of(&name));
        // a font that is not installed: the default, and the user is told once
        cfg.all.family = Some("No Such Font".into());
        let told = st.sync(&ctx, &cfg).expect("told");
        assert!(told.contains("No Such Font") && told.contains("not installed"), "{told}");
        assert!(st.installed.is_empty());
        pass(&ctx);
        assert_eq!(st.sync(&ctx, &cfg), None, "said once");
        assert_eq!(st.area(Area::Ui).family, FontFamily::Proportional);
        cfg.all.size = 23.0;
        assert_eq!(st.sync(&ctx, &cfg), None, "and not again for a size");
    }

    // spec: CFG-036
    #[test]
    fn cfg_036_a_chosen_font_waits_for_the_search_of_the_installed_fonts() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let (db, name) = virgil_db();
        let mut st = FontState::new();
        st.waiting = false;
        let cfg = FontsCfg { all: FontChoice { family: Some(name.clone()), size: 18.0 }, ..FontsCfg::default() };
        // the search runs in a thread of its own: hand over the result by the channel it uses
        let (tx, rx) = mpsc::channel();
        st.system.rx = Some(rx);
        st.system.started = true;
        assert_eq!(st.sync(&ctx, &cfg), None);
        assert!(st.waiting);
        assert_eq!((st.area(Area::Ui).size, st.area(Area::Ui).family), (18.0, FontFamily::Proportional), "the size at once, the font when it is known");
        tx.send(db).unwrap();
        assert_eq!(st.sync(&ctx, &cfg), None);
        assert!(!st.waiting && st.installed.contains(&name));
        pass(&ctx);
        st.sync(&ctx, &cfg);
        assert_eq!(st.area(Area::Ui).family, family_of(&name));
    }

    // spec: CFG-033
    #[test]
    fn cfg_033_an_area_has_its_text_styles_and_gives_hand_sized_text_its_size() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            let big = AreaFont { family: FontFamily::Proportional, size: 28.0 };
            assert_eq!((px(10.0), font(10.0).size), (10.0, 10.0));
            with_area(ui, &big, |ui| {
                assert_eq!(ui.style().text_styles[&egui::TextStyle::Body].size, 28.0);
                assert_eq!(ui.style().text_styles[&egui::TextStyle::Small].size, 23.0);
                assert_eq!(ui.style().text_styles[&egui::TextStyle::Monospace].family, FontFamily::Monospace);
                assert_eq!((px(10.0), font(10.0).size), (20.0, 20.0));
            });
            assert_eq!(ui.style().text_styles[&egui::TextStyle::Body].size, 14.0, "the area ends with its ui");
            assert_eq!(px(10.0), 10.0);
        });
        out.textures_delta.clear();
    }
}
