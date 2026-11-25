//! Fonts and text sizes (CFG-030 to CFG-040): one font and size for each area of the window,
//! the system fonts to choose from, and the egui fonts and text styles that follow from the
//! settings.
//!
//! The settings are `Config::fonts` ([`FontsCfg`]). Every frame [`FontState::sync`] compares them
//! with what is in use and, when they differ, loads the chosen system fonts into egui, so a change
//! of the settings (in the dialog or in the settings file) is used at once and fonts are only
//! rebuilt when the set of chosen families changes.

// The fonts of the exports (ADR-0040, #658): the licence of a font file, the scan of the fonts
// of the vault and the system, and the resolver that decides which font an export uses.
pub mod field;
#[cfg(test)]
mod field_tests;
pub mod licence;
#[cfg(test)]
mod licence_tests;
pub mod resolve;
#[cfg(test)]
mod resolve_tests;
pub mod scan;
#[cfg(test)]
mod scan_tests;
pub mod sfnt;
#[cfg(test)]
mod sfnt_tests;
#[cfg(test)]
pub(crate) mod test_fonts;

use crate::font_list::ListFonts;
use crate::theme;
use eframe::egui::{self, FontFamily, FontId, Ui};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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

/// A font that can be used in egui: the family, the size in points, and which faces of its
/// own the family has for bold and italic text (CFG-039).
#[derive(Clone, Debug, PartialEq)]
pub struct AreaFont {
    pub family: FontFamily,
    pub size: f32,
    pub faces: Faces,
}

/// The faces a family has besides its regular one (CFG-039). Shale's own font has none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Faces {
    pub bold: bool,
    pub italic: bool,
    pub bold_italic: bool,
}

impl Faces {
    fn has(self, v: Variant) -> bool {
        match v {
            Variant::Regular => true,
            Variant::Bold => self.bold,
            Variant::Italic => self.italic,
            Variant::BoldItalic => self.bold_italic,
        }
    }
}

/// A face of a family (CFG-039).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Variant {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl Variant {
    const ALL: [Variant; 4] = [Variant::Regular, Variant::Bold, Variant::Italic, Variant::BoldItalic];

    fn suffix(self) -> &'static str {
        match self {
            Variant::Regular => "",
            Variant::Bold => ":bold",
            Variant::Italic => ":italic",
            Variant::BoldItalic => ":bold-italic",
        }
    }

    fn bold(self) -> bool {
        matches!(self, Variant::Bold | Variant::BoldItalic)
    }

    fn italic(self) -> bool {
        matches!(self, Variant::Italic | Variant::BoldItalic)
    }
}

/// How bold or italic text is drawn (CFG-039): in `family`, and with egui's imitation of bold
/// (`fake_bold`: extra letter spacing) or italic (`fake_italic`: slanted) where the family has
/// no face of its own for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Styled {
    pub family: FontFamily,
    pub fake_bold: bool,
    pub fake_italic: bool,
}

impl AreaFont {
    /// Shale's own font at `size`.
    pub fn shale(size: f32) -> AreaFont {
        AreaFont { family: FontFamily::Proportional, size, faces: Faces::default() }
    }

    /// The family and imitation for text that is `bold` and/or `italic` (CFG-039): the face that
    /// has both, else the one that has the most of it, with the rest imitated.
    pub fn styled(&self, bold: bool, italic: bool) -> Styled {
        let wanted = match (bold, italic) {
            (true, true) => Variant::BoldItalic,
            (true, false) => Variant::Bold,
            (false, true) => Variant::Italic,
            (false, false) => Variant::Regular,
        };
        let order: &[Variant] = match wanted {
            Variant::BoldItalic => &[Variant::BoldItalic, Variant::Bold, Variant::Italic, Variant::Regular],
            v => &[v, Variant::Regular],
        };
        let v = order.iter().copied().find(|v| self.faces.has(*v)).unwrap_or(Variant::Regular);
        let family = match (&self.family, v) {
            (FontFamily::Name(n), v) if v != Variant::Regular => FontFamily::Name(format!("{n}{}", v.suffix()).into()),
            (f, _) => f.clone(),
        };
        Styled { family, fake_bold: bold && !v.bold(), fake_italic: italic && !v.italic() }
    }
}

/// The two fonts of the editor (CFG-032).
#[derive(Clone, Debug, PartialEq)]
pub struct EditorFonts {
    pub raw: AreaFont,
    pub rendered: AreaFont,
}

impl EditorFonts {
    /// One size in Shale's own font for both (the outline, tests).
    pub fn uniform(size: f32) -> EditorFonts {
        let f = AreaFont::shale(size);
        EditorFonts { raw: f.clone(), rendered: f }
    }
}

/// How often the fonts of this context were set (CFG-077).
#[cfg(test)]
pub(crate) fn set_fonts_calls(ctx: &egui::Context) -> usize {
    ctx.data(|d| d.get_temp::<usize>(egui::Id::NULL.with("fonts-set-calls"))).unwrap_or(0)
}

/// Set the fonts of egui and remember in which pass, for what is laid out with them and kept ([`settled`]).
pub fn set_fonts(ctx: &egui::Context, definitions: egui::FontDefinitions) {
    #[cfg(test)]
    {
        let n = set_fonts_calls(ctx);
        ctx.data_mut(|d| d.insert_temp(egui::Id::NULL.with("fonts-set-calls"), n + 1));
    }
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(egui::Id::NULL.with("fonts-set-in-pass"), pass));
    ctx.set_fonts(definitions);
}

/// Which fonts egui lays out with in this pass, as a number that changes with them; `None` in the
/// pass that sets them, when some text may still be laid out with the old ones (EDIT-106).
pub fn settled(ctx: &egui::Context) -> Option<u64> {
    let set = ctx.data(|d| d.get_temp::<u64>(egui::Id::NULL.with("fonts-set-in-pass"))).unwrap_or(0);
    (ctx.cumulative_pass_nr() > set).then_some(set)
}

/// The name of the egui family (and font data) that holds the face `v` of the system font `name`.
fn key_of(name: &str, v: Variant) -> String {
    format!("system:{name}{}", v.suffix())
}

/// The egui family that holds the system font `name`.
fn family_of(name: &str) -> FontFamily {
    FontFamily::Name(key_of(name, Variant::Regular).into())
}

/// The font of every area as egui uses it.
#[derive(Clone, Debug, PartialEq)]
struct Resolved([AreaFont; 6]);

impl Default for Resolved {
    fn default() -> Self {
        Resolved(Area::ALL.map(|a| AreaFont::shale(a.default_size())))
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

    /// A face of a family: the bytes of its file and the index of the face in it. A bold or
    /// italic face only when the family has one of its own (CFG-039): not the regular face that
    /// the search offers in its place, nor a face that is not what was asked for.
    fn face(&self, name: &str, v: Variant) -> Option<(Vec<u8>, u32)> {
        face_in(self.db.as_ref()?, name, v)
    }

    /// The installed fonts, for a thread that reads faces (CFG-042).
    pub fn database(&self) -> Option<Arc<fontdb::Database>> {
        self.db.clone()
    }
}

/// A face of a family in `db` (see [`SystemFonts::face`]).
fn face_in(db: &fontdb::Database, name: &str, v: Variant) -> Option<(Vec<u8>, u32)> {
    let find = |v: Variant| {
        let weight = if v.bold() { fontdb::Weight::BOLD } else { fontdb::Weight::NORMAL };
        let style = if v.italic() { fontdb::Style::Italic } else { fontdb::Style::Normal };
        db.query(&fontdb::Query { families: &[fontdb::Family::Name(name)], weight, stretch: fontdb::Stretch::Normal, style })
    };
    let id = find(v)?;
    if v != Variant::Regular {
        let info = db.face(id)?;
        let fits = (!v.bold() || info.weight.0 >= 600) && (!v.italic() || info.style != fontdb::Style::Normal);
        if !fits || find(Variant::Regular) == Some(id) {
            return None;
        }
    }
    db.with_face_data(id, |data, index| {
        let mut bytes = data.to_vec();
        hide_mono_bitmaps(&mut bytes);
        (bytes, index)
    })
}

/// The regular face of `name` as egui font data, for a font that is only listed (CFG-042).
pub fn regular_face(db: &fontdb::Database, name: &str) -> Option<(Vec<u8>, u32)> {
    face_in(db, name, Variant::Regular)
}

/// Hide the embedded black-and-white bitmaps (`EBDT`, `EBLC`) of every face in `font` that has
/// outlines (CFG-038): egui takes them for colour glyphs and draws those letters white whatever
/// colour the text has. The tags are renamed in place to ones that sort the same, so the table
/// directory stays in order and the outlines are used. Anything that is not a font is left alone.
pub fn hide_mono_bitmaps(font: &mut [u8]) {
    let u32_at = |b: &[u8], at: usize| b.get(at..at + 4).map(|x| u32::from_be_bytes([x[0], x[1], x[2], x[3]]));
    let faces: Vec<usize> = if font.starts_with(b"ttcf") {
        let n = u32_at(font, 8).unwrap_or(0) as usize;
        (0..n.min(4096)).filter_map(|i| u32_at(font, 12 + 4 * i).map(|o| o as usize)).collect()
    } else {
        vec![0]
    };
    for face in faces {
        let Some(head) = font.get(face..face + 6) else { continue };
        if !matches!(&head[..4], [0, 1, 0, 0] | b"true" | b"OTTO") {
            continue;
        }
        let n = u16::from_be_bytes([head[4], head[5]]) as usize;
        let tag_at = |i: usize| face + 12 + 16 * i;
        let Some(dir) = font.get(face + 12..tag_at(n)) else { continue };
        let (entries, _) = dir.as_chunks::<16>();
        if !entries.iter().any(|e| matches!(&e[..4], b"glyf" | b"CFF " | b"CFF2")) {
            continue; // a bitmap-only font: its bitmaps are its letters
        }
        for i in 0..n {
            let at = tag_at(i);
            let renamed: Option<&[u8; 4]> = match &font[at..at + 4] {
                b"EBDT" => Some(b"EBDt"),
                b"EBLC" => Some(b"EBLc"),
                _ => None,
            };
            if let Some(r) = renamed {
                font[at..at + 4].copy_from_slice(r);
            }
        }
    }
}

/// A system font that is loaded: its name, the faces it has besides the regular one, and the
/// font data of each face it has, the regular one first (CFG-039).
struct Loaded {
    name: String,
    faces: Faces,
    data: Vec<(Variant, Arc<egui::FontData>)>,
}

/// How many fonts that are only shown as a sample stay loaded (CFG-040).
pub const PREVIEWS: usize = 8;

/// The font a sample text can be drawn in (CFG-040).
#[derive(Clone, Debug, PartialEq)]
pub enum Preview {
    Ready(AreaFont),
    /// it is being loaded, or the installed fonts are still being searched for
    Loading,
    /// no such font is installed
    Missing,
}

/// How an entry of the font picker is drawn (CFG-077).
#[derive(Clone, Debug, PartialEq)]
pub enum ListLook {
    /// its face is not in egui (yet): the name in Shale's font
    Fallback,
    /// its face is in egui; `symbol`: its letters are pictures, so the name stays in Shale's font
    /// and a sample in the font follows
    Own { family: FontFamily, symbol: bool },
}

/// What is in use, and what the settings ask for (CFG-036).
pub struct FontState {
    pub system: SystemFonts,
    resolved: Resolved,
    /// Fonts handed to egui in this pass are used from the next one on: what the areas use then,
    /// and the faces of every family that can be drawn in.
    pending: Option<(Resolved, BTreeMap<String, Faces>, u64)>,
    /// The system families egui can draw in now, and their faces.
    usable: BTreeMap<String, Faces>,
    /// Fonts loaded only for a sample text (CFG-040), the one asked for last at the end.
    previews: Vec<String>,
    /// A font for a sample was asked for that is not loaded, or the faces of the picker's rows
    /// changed.
    previews_changed: bool,
    /// The faces of the entries of the font picker (CFG-077).
    list: ListFonts,
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
        FontState {
            system: SystemFonts::default(),
            resolved: Resolved::default(),
            pending: None,
            usable: BTreeMap::new(),
            previews: Vec::new(),
            previews_changed: false,
            list: ListFonts::default(),
            seen: None,
            waiting: false,
            installed: BTreeSet::new(),
            told: BTreeSet::new(),
        }
    }

    /// The font of a sample text of the system font `name` at `size` (CFG-040); `None` is
    /// Shale's own font. A font that is installed but not loaded is loaded by the next `sync`s,
    /// besides the ones the areas use and without changing them; the last `PREVIEWS` of them
    /// stay loaded.
    pub fn preview(&mut self, name: Option<&str>, size: f32) -> Preview {
        let Some(name) = name else { return Preview::Ready(AreaFont::shale(size)) };
        if let Some(i) = self.previews.iter().position(|p| p == name) {
            let p = self.previews.remove(i);
            self.previews.push(p);
        }
        if let Some(faces) = self.usable.get(name) {
            return Preview::Ready(AreaFont { family: family_of(name), size, faces: *faces });
        }
        if !self.system.ready() {
            self.system.start();
            return Preview::Loading;
        }
        if !self.system.families().iter().any(|f| f == name) {
            return Preview::Missing;
        }
        if !self.previews.iter().any(|p| p == name) {
            self.previews.push(name.to_string());
            if self.previews.len() > PREVIEWS {
                self.previews.remove(0);
            }
            self.previews_changed = true;
        }
        Preview::Loading
    }

    /// The entries of the font picker that are on screen in this frame (CFG-077): their faces
    /// are read and handed to egui once the list rests.
    pub fn list_want(&mut self, names: Vec<String>, now: f64) {
        self.list.want(names, now);
    }

    /// How an entry of the font picker is drawn (CFG-077).
    pub fn list_look(&self, name: &str) -> ListLook {
        match self.list.included().iter().find(|f| f.name == name) {
            Some(f) if self.usable.contains_key(name) => ListLook::Own { family: family_of(name), symbol: f.symbol },
            _ => ListLook::Fallback,
        }
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
        let now = ctx.input(|i| i.time);
        self.sync_at(ctx, cfg, now)
    }

    /// [`FontState::sync`] at the time `now` (in seconds).
    fn sync_at(&mut self, ctx: &egui::Context, cfg: &FontsCfg, now: f64) -> Option<String> {
        if let Some((r, usable, pass)) = self.pending.take() {
            if ctx.cumulative_pass_nr() > pass {
                self.use_resolved(ctx, r);
                self.usable = usable;
            } else {
                self.pending = Some((r, usable, pass));
            }
        }
        let was_ready = self.system.ready();
        self.system.poll();
        self.previews_changed |= self.list.step(ctx, self.system.database().as_ref(), now);
        let arrived = self.system.ready() && !was_ready;
        let wanted = cfg.families();
        if self.seen.as_ref() == Some(cfg) && !(self.waiting && arrived) && !self.previews_changed {
            if self.waiting {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            return None;
        }
        self.seen = Some(cfg.clone());
        self.previews_changed = false;
        if !wanted.is_empty() {
            self.system.start();
        }
        self.waiting = !wanted.is_empty() && !self.system.ready();
        let mut resolved = Resolved::default();
        let mut missing = BTreeSet::new();
        // each family once: its faces, and the font data of each (the regular one first)
        let mut loaded: Vec<Loaded> = Vec::new();
        for (i, area) in Area::ALL.iter().enumerate() {
            let c = cfg.choice(*area);
            resolved.0[i].size = c.size;
            let Some(name) = c.family else { continue };
            if !self.system.ready() {
                continue;
            }
            if let Some(l) = loaded.iter().find(|l| l.name == name) {
                resolved.0[i].family = family_of(&name);
                resolved.0[i].faces = l.faces;
            } else if let Some(l) = self.load_family(&name) {
                resolved.0[i].family = family_of(&name);
                resolved.0[i].faces = l.faces;
                loaded.push(l);
            } else {
                missing.insert(name);
            }
        }
        // the fonts of the sample texts (CFG-040), which no area uses
        if self.system.ready() {
            for name in &self.previews {
                if !loaded.iter().any(|l| l.name == *name) {
                    loaded.extend(self.load_family(name));
                }
            }
        }
        // the faces of the picker's entries (CFG-077): the regular face only, all in one batch
        for f in self.list.included() {
            if !loaded.iter().any(|l| l.name == f.name) {
                loaded.push(Loaded { name: f.name.clone(), faces: Faces::default(), data: vec![(Variant::Regular, f.data.clone())] });
            }
        }
        let now: BTreeSet<String> = loaded.iter().map(|l| l.name.clone()).collect();
        let usable: BTreeMap<String, Faces> = loaded.iter().map(|l| (l.name.clone(), l.faces)).collect();
        if self.waiting {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if now != self.installed {
            set_fonts(ctx, theme::font_definitions(families_for_egui(loaded)));
            self.installed = now;
            self.pending = Some((resolved, usable, ctx.cumulative_pass_nr()));
            ctx.request_repaint();
        } else {
            self.use_resolved(ctx, resolved);
            self.usable = usable;
        }
        let fresh: Vec<String> = missing.difference(&self.told).cloned().collect();
        self.told = missing;
        if fresh.is_empty() {
            None
        } else {
            Some(format!("The font {} is not installed, so Shale's own font is used instead", fresh.iter().map(|f| format!("\"{f}\"")).collect::<Vec<_>>().join(", ")))
        }
    }

    /// The faces of the system font `name` that it has (CFG-039), the regular one first; none
    /// when it has no regular face.
    fn load_family(&self, name: &str) -> Option<Loaded> {
        let mut faces = Faces::default();
        let mut data = Vec::new();
        for v in Variant::ALL {
            let Some((bytes, index)) = self.system.face(name, v) else {
                if v == Variant::Regular {
                    return None;
                }
                continue;
            };
            let mut d = egui::FontData::from_owned(bytes);
            d.index = index;
            data.push((v, Arc::new(d)));
            match v {
                Variant::Regular => {}
                Variant::Bold => faces.bold = true,
                Variant::Italic => faces.italic = true,
                Variant::BoldItalic => faces.bold_italic = true,
            }
        }
        Some(Loaded { name: name.to_string(), faces, data })
    }

    fn use_resolved(&mut self, ctx: &egui::Context, r: Resolved) {
        if r.get(Area::Ui) != self.resolved.get(Area::Ui) {
            theme::set_text_styles(ctx, r.get(Area::Ui));
        }
        self.resolved = r;
    }
}

/// The fonts of the loaded families as egui takes them: every face as font data of its own,
/// and every face as a family that falls back to the regular face of its font (CFG-039).
fn families_for_egui(loaded: Vec<Loaded>) -> theme::SystemFonts {
    let mut out = theme::SystemFonts::default();
    for l in loaded {
        let regular = key_of(&l.name, Variant::Regular);
        for (v, data) in l.data {
            let key = key_of(&l.name, v);
            let chain = if v == Variant::Regular { vec![key.clone()] } else { vec![key.clone(), regular.clone()] };
            out.families.push((key.clone(), chain));
            out.data.push((key, data));
        }
    }
    out
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

/// A size of the 14-point design of the dialogs, in the size of the area that is drawn.
pub fn px(size: f32) -> f32 {
    ACTIVE.with(|a| a.borrow().as_ref().map_or(size, |f| size * f.size / UI_SIZE))
}

/// How bold or italic text is drawn in the area that is being drawn (CFG-039).
pub fn styled(bold: bool, italic: bool) -> Styled {
    ACTIVE.with(|a| a.borrow().as_ref().map_or_else(|| AreaFont::shale(UI_SIZE).styled(bold, italic), |f| f.styled(bold, italic)))
}

/// Whether `family` is the bold and/or italic face of a system font (CFG-039): text in such a
/// face has no imitation that says so, so the PDF export asks here.
pub fn bold_italic_of(family: &FontFamily) -> (bool, bool) {
    let FontFamily::Name(n) = family else { return (false, false) };
    let v = [Variant::BoldItalic, Variant::Bold, Variant::Italic].into_iter().find(|v| n.starts_with("system:") && n.ends_with(v.suffix()));
    v.map_or((false, false), |v| (v.bold(), v.italic()))
}

/// A font of `size` points of the 14-point design, in the font and size of the area that is drawn.
pub fn font(size: f32) -> FontId {
    ACTIVE.with(|a| a.borrow().as_ref().map_or_else(|| FontId::proportional(size), |f| FontId::new(size * f.size / UI_SIZE, f.family.clone())))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::theme;

    /// A font file of the given kind with these tables in its directory (offsets and lengths 0).
    fn sfnt(kind: &[u8; 4], tags: &[&[u8; 4]]) -> Vec<u8> {
        let mut b = kind.to_vec();
        b.extend_from_slice(&(tags.len() as u16).to_be_bytes());
        b.extend_from_slice(&[0; 6]);
        for t in tags {
            b.extend_from_slice(*t);
            b.extend_from_slice(&[0; 12]);
        }
        b
    }

    fn tags(font: &[u8], face: usize) -> Vec<String> {
        let n = u16::from_be_bytes([font[face + 4], font[face + 5]]) as usize;
        (0..n).map(|i| String::from_utf8_lossy(&font[face + 12 + 16 * i..face + 16 + 16 * i]).into_owned()).collect()
    }

    // spec: CFG-038
    #[test]
    fn cfg_038_the_mono_bitmaps_of_an_outline_font_are_hidden_and_the_order_stays() {
        let mut f = sfnt(&[0, 1, 0, 0], &[b"DSIG", b"EBDT", b"EBLC", b"EBSC", b"GDEF", b"glyf"]);
        hide_mono_bitmaps(&mut f);
        let t = tags(&f, 0);
        assert_eq!(t, ["DSIG", "EBDt", "EBLc", "EBSC", "GDEF", "glyf"]);
        let mut sorted = t.clone();
        sorted.sort();
        assert_eq!(t, sorted, "the directory stays sorted, so a binary search still finds every table");
        // CFF outlines count as outlines too
        for kind in [b"OTTO", b"true"] {
            let mut f = sfnt(kind, &[b"CFF ", b"EBDT", b"EBLC"]);
            hide_mono_bitmaps(&mut f);
            assert_eq!(tags(&f, 0), ["CFF ", "EBDt", "EBLc"]);
        }
    }

    // spec: CFG-038
    #[test]
    fn cfg_038_colour_glyphs_bitmap_only_fonts_and_other_bytes_are_left_alone() {
        let colour = sfnt(&[0, 1, 0, 0], &[b"CBDT", b"CBLC", b"COLR", b"glyf", b"sbix"]);
        let mut f = colour.clone();
        hide_mono_bitmaps(&mut f);
        assert_eq!(f, colour, "real colour glyphs stay");
        let bitmaps_only = sfnt(&[0, 1, 0, 0], &[b"EBDT", b"EBLC", b"cmap"]);
        let mut f = bitmaps_only.clone();
        hide_mono_bitmaps(&mut f);
        assert_eq!(f, bitmaps_only, "a font that is only bitmaps keeps them");
        for junk in [vec![], b"wOFF0000".to_vec(), vec![0, 1, 0, 0, 0xff, 0xff, 0, 0], b"ttcf\0\x01\0\0\x7f\xff\xff\xff".to_vec()] {
            let mut f = junk.clone();
            hide_mono_bitmaps(&mut f);
            assert_eq!(f, junk, "no font, a cut-off directory or a broken collection: unchanged, no panic");
        }
    }

    // spec: CFG-038
    #[test]
    fn cfg_038_every_face_of_a_collection_is_treated() {
        let a = sfnt(&[0, 1, 0, 0], &[b"EBDT", b"EBLC", b"glyf"]);
        let b = sfnt(&[0, 1, 0, 0], &[b"EBDT", b"cmap"]);
        let mut f = b"ttcf".to_vec();
        f.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 2]);
        let first = 20;
        f.extend_from_slice(&(first as u32).to_be_bytes());
        f.extend_from_slice(&((first + a.len()) as u32).to_be_bytes());
        f.extend_from_slice(&a);
        f.extend_from_slice(&b);
        hide_mono_bitmaps(&mut f);
        assert_eq!(tags(&f, first), ["EBDt", "EBLc", "glyf"]);
        assert_eq!(tags(&f, first + a.len()), ["EBDT", "cmap"], "the second face has no outlines");
    }

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

    /// A copy of the bundled Virgil font that says it has `weight` and is italic or not (its
    /// `OS/2` table): a family of real faces for the tests, whatever is installed (CFG-039).
    fn virgil_as(weight: u16, italic: bool) -> Vec<u8> {
        let mut f = include_bytes!("../assets/fonts/Virgil.otf").to_vec();
        let n = u16::from_be_bytes([f[4], f[5]]) as usize;
        let entry = (0..n).map(|i| 12 + 16 * i).find(|e| &f[*e..*e + 4] == b"OS/2").expect("Virgil has an OS/2 table");
        let os2 = u32::from_be_bytes([f[entry + 8], f[entry + 9], f[entry + 10], f[entry + 11]]) as usize;
        f[os2 + 4..os2 + 6].copy_from_slice(&weight.to_be_bytes());
        let mut sel = u16::from_be_bytes([f[os2 + 62], f[os2 + 63]]) & !(1 | 1 << 6 | 1 << 9);
        sel |= if italic { 1 } else { 1 << 6 };
        f[os2 + 62..os2 + 64].copy_from_slice(&sel.to_be_bytes());
        f
    }

    /// The Virgil family with the faces of `faces`: (weight, italic) each.
    pub(crate) fn virgil_family(faces: &[(u16, bool)]) -> (fontdb::Database, String) {
        let mut db = fontdb::Database::new();
        for (w, it) in faces {
            db.load_font_data(virgil_as(*w, *it));
        }
        let name = virgil_db().1;
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
        let (bytes, index) = s.face(&name, Variant::Regular).expect("the face loads");
        assert!(bytes.len() > 1000 && index == 0);
        assert!(s.face("No Such Family", Variant::Regular).is_none());
    }

    // spec: CFG-033, CFG-035
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
            let big = AreaFont::shale(28.0);
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

    // spec: CFG-039
    #[test]
    fn cfg_039_a_family_s_own_bold_and_italic_faces_are_loaded_and_stand_ins_are_not() {
        let weight = |b: &[u8]| {
            let mut db = fontdb::Database::new();
            db.load_font_data(b.to_vec());
            let f = db.faces().next().unwrap();
            (f.weight.0, f.style)
        };
        let (db, name) = virgil_family(&[(400, false), (700, false), (400, true)]);
        let mut s = SystemFonts::default();
        s.set(db);
        assert_eq!(s.families(), std::slice::from_ref(&name), "one family, however many faces");
        assert_eq!(weight(&s.face(&name, Variant::Regular).unwrap().0), (400, fontdb::Style::Normal));
        assert_eq!(weight(&s.face(&name, Variant::Bold).unwrap().0), (700, fontdb::Style::Normal));
        assert_eq!(weight(&s.face(&name, Variant::Italic).unwrap().0), (400, fontdb::Style::Italic));
        assert!(s.face(&name, Variant::BoldItalic).is_none(), "the italic face the search offers is not bold");
        // only a regular face: the search offers it for everything, and it is not taken
        let (db, name) = virgil_family(&[(400, false)]);
        s.set(db);
        assert!(s.face(&name, Variant::Regular).is_some());
        assert!([Variant::Bold, Variant::Italic, Variant::BoldItalic].iter().all(|v| s.face(&name, *v).is_none()));
        // only a bold face: it is the regular one too, and not a second time the bold one
        let (db, name) = virgil_family(&[(700, false)]);
        s.set(db);
        assert_eq!(weight(&s.face(&name, Variant::Regular).unwrap().0).0, 700);
        assert!(s.face(&name, Variant::Bold).is_none());
        // a semibold face counts as bold, a medium one does not
        let (db, name) = virgil_family(&[(400, false), (600, false)]);
        s.set(db);
        assert!(s.face(&name, Variant::Bold).is_some());
        let (db, name) = virgil_family(&[(400, false), (500, false)]);
        s.set(db);
        assert!(s.face(&name, Variant::Bold).is_none());
    }

    // spec: CFG-039
    #[test]
    fn cfg_039_bold_and_italic_take_the_faces_there_are_and_imitate_the_rest() {
        let fam = |n: &str| FontFamily::Name(n.into());
        let font = |bold, italic, bold_italic| AreaFont { family: fam("system:X"), size: 15.0, faces: Faces { bold, italic, bold_italic } };
        let st = |f: &AreaFont, b, i| {
            let s = f.styled(b, i);
            (s.family, s.fake_bold, s.fake_italic)
        };
        let all = font(true, true, true);
        assert_eq!(st(&all, false, false), (fam("system:X"), false, false));
        assert_eq!(st(&all, true, false), (fam("system:X:bold"), false, false));
        assert_eq!(st(&all, false, true), (fam("system:X:italic"), false, false));
        assert_eq!(st(&all, true, true), (fam("system:X:bold-italic"), false, false));
        // no bold-italic face: the bold one with a slant, else the italic one made bold
        assert_eq!(st(&font(true, true, false), true, true), (fam("system:X:bold"), false, true));
        assert_eq!(st(&font(false, true, false), true, true), (fam("system:X:italic"), true, false));
        // no faces at all, and Shale's own font: everything imitated as before
        assert_eq!(st(&font(false, false, false), true, true), (fam("system:X"), true, true));
        let shale = AreaFont::shale(15.0);
        assert_eq!(st(&shale, true, true), (FontFamily::Proportional, true, true));
        assert_eq!(st(&shale, false, false), (FontFamily::Proportional, false, false));
        // the area that is drawn says which faces there are
        assert_eq!(styled(true, false), Styled { family: FontFamily::Proportional, fake_bold: true, fake_italic: false });
        with_family(&all, || assert_eq!(styled(true, false), Styled { family: fam("system:X:bold"), fake_bold: false, fake_italic: false }));
        // the PDF export finds out from the family what the face is
        assert_eq!(bold_italic_of(&fam("system:X:bold")), (true, false));
        assert_eq!(bold_italic_of(&fam("system:X:italic")), (false, true));
        assert_eq!(bold_italic_of(&fam("system:X:bold-italic")), (true, true));
        assert_eq!(bold_italic_of(&fam("system:X")), (false, false));
        assert_eq!(bold_italic_of(&FontFamily::Proportional), (false, false));
        assert_eq!(bold_italic_of(&fam(theme::HAND_FONT)), (false, false));
    }

    // spec: CFG-039
    #[test]
    fn cfg_039_the_faces_of_a_chosen_font_are_loaded_into_egui_and_text_is_laid_out_in_them() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let (db, name) = virgil_family(&[(400, false), (700, false), (400, true), (700, true)]);
        let mut st = FontState::new();
        st.system.set(db);
        let cfg = FontsCfg { all: FontChoice { family: Some(name.clone()), size: 16.0 }, ..FontsCfg::default() };
        assert_eq!(st.sync(&ctx, &cfg), None);
        pass(&ctx);
        st.sync(&ctx, &cfg);
        let ui = st.area(Area::Ui);
        assert_eq!(ui.faces, Faces { bold: true, italic: true, bold_italic: true });
        assert_eq!(st.area(Area::EditorRendered).faces, ui.faces, "every area of the family");
        for (b, i) in [(true, false), (false, true), (true, true)] {
            let family = ui.styled(b, i).family;
            let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))), ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                ui.label(egui::RichText::new("Bold text").family(family.clone()));
            });
            out.textures_delta.clear();
        }
        // a family with no faces of its own: none are said to be there
        let (db, name) = virgil_family(&[(400, false)]);
        let mut st = FontState::new();
        st.system.set(db);
        let cfg = FontsCfg { all: FontChoice { family: Some(name), size: 16.0 }, ..FontsCfg::default() };
        st.sync(&ctx, &cfg);
        pass(&ctx);
        st.sync(&ctx, &cfg);
        assert_eq!(st.area(Area::Ui).faces, Faces::default());
    }

    // spec: EDIT-106
    #[test]
    fn the_pass_that_sets_the_fonts_is_not_settled_and_the_later_ones_name_it() {
        let ctx = egui::Context::default();
        let mut seen = Vec::new();
        for pass in 0..3 {
            let mut done = ctx.run_ui(egui::RawInput::default(), |ui| {
                if pass == 1 {
                    set_fonts(ui.ctx(), egui::FontDefinitions::default());
                }
                seen.push((settled(ui.ctx()), ui.ctx().cumulative_pass_nr()));
            });
            done.textures_delta.clear();
        }
        let at = seen[1].1;
        assert_eq!(seen[1].0, None, "in the pass that sets the fonts nothing is kept");
        assert_eq!(seen[2].0, Some(at), "afterwards the pass that set them is the number of the fonts");
    }

    // spec: CFG-040
    #[test]
    fn cfg_040_a_font_for_a_sample_is_loaded_besides_the_chosen_ones_and_changes_no_area() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let (mut db, virgil) = virgil_db();
        db.load_font_data(epaint_default_fonts::HACK_REGULAR.to_vec());
        let hack = db.faces().filter_map(|f| f.families.first().map(|(n, _)| n.clone())).find(|n| *n != virgil).expect("a second family");
        let mut st = FontState::new();
        st.system.set(db);
        let cfg = FontsCfg { all: FontChoice { family: Some(virgil.clone()), size: 16.0 }, ..FontsCfg::default() };
        st.sync(&ctx, &cfg);
        pass(&ctx);
        st.sync(&ctx, &cfg);
        // Shale's own font and a chosen font are there at once, at the size asked for
        assert_eq!(st.preview(None, 20.0), Preview::Ready(AreaFont::shale(20.0)));
        assert_eq!(st.preview(Some(&virgil), 12.0), Preview::Ready(AreaFont { family: family_of(&virgil), size: 12.0, faces: Faces::default() }));
        assert_eq!(st.preview(Some("No Such Font"), 12.0), Preview::Missing);
        // another installed font is loaded by the next syncs, and no area takes it
        assert_eq!(st.preview(Some(&hack), 12.0), Preview::Loading);
        assert_eq!(st.preview(Some(&hack), 12.0), Preview::Loading, "asked for once");
        assert_eq!(st.previews, std::slice::from_ref(&hack));
        st.sync(&ctx, &cfg);
        assert_eq!(st.preview(Some(&hack), 12.0), Preview::Loading, "egui has it from the next pass on");
        pass(&ctx);
        st.sync(&ctx, &cfg);
        assert_eq!(st.preview(Some(&hack), 12.0), Preview::Ready(AreaFont { family: family_of(&hack), size: 12.0, faces: Faces::default() }));
        assert!(Area::ALL.iter().all(|a| st.area(*a).family == family_of(&virgil)), "the areas keep the chosen font");
        assert_eq!(st.installed, BTreeSet::from([virgil.clone(), hack.clone()]));
        // the text can be laid out in it
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            ui.label(egui::RichText::new("sample").family(family_of(&hack)));
        });
        out.textures_delta.clear();
        assert_eq!(st.sync(&ctx, &cfg), None, "nothing to tell about a sample");
    }

    // spec: CFG-040
    #[test]
    fn cfg_040_only_the_last_fonts_of_samples_stay_and_a_sample_waits_for_the_search() {
        let mut st = FontState::new();
        // the search is still running: wait, and do not start a second one
        let (tx, rx) = mpsc::channel();
        st.system.rx = Some(rx);
        st.system.started = true;
        assert_eq!(st.preview(Some("A"), 14.0), Preview::Loading);
        assert!(st.previews.is_empty());
        // many installed fonts (names only: none of them loads, which is not the point here)
        st.system.db = Some(Arc::new(fontdb::Database::new()));
        st.system.families = (0..12).map(|i| format!("F{i}")).collect();
        drop(tx);
        for i in 0..12 {
            assert_eq!(st.preview(Some(&format!("F{i}")), 14.0), Preview::Loading);
        }
        assert_eq!(st.previews.len(), PREVIEWS);
        assert_eq!(st.previews.first().map(String::as_str), Some("F4"), "the oldest are dropped");
        // asking again for one that is kept moves it to the end, so it is dropped last
        st.preview(Some("F5"), 14.0);
        assert_eq!(st.previews.last().map(String::as_str), Some("F5"));
        st.preview(Some("F0"), 14.0);
        assert!(st.previews.contains(&"F5".to_string()) && !st.previews.contains(&"F4".to_string()));
    }

    /// A database of three families, and their names.
    fn three_families() -> (fontdb::Database, Vec<String>) {
        let (mut db, _) = virgil_db();
        db.load_font_data(epaint_default_fonts::HACK_REGULAR.to_vec());
        db.load_font_data(epaint_default_fonts::UBUNTU_LIGHT.to_vec());
        let mut s = SystemFonts::default();
        s.set(db);
        let names = s.families().to_vec();
        (s.db.take().and_then(|d| Arc::try_unwrap(d).ok()).expect("the only owner"), names)
    }

    /// One frame of the picker at time `t` with `names` on screen (none: the picker is closed).
    fn list_frame(st: &mut FontState, ctx: &egui::Context, cfg: &FontsCfg, t: f64, names: &[String]) {
        if !names.is_empty() {
            st.list_want(names.to_vec(), t);
        }
        st.sync_at(ctx, cfg, t);
        pass(ctx);
    }

    // spec: CFG-077
    #[test]
    fn cfg_077_a_face_is_loaded_for_the_rows_on_screen_in_one_batch_and_the_row_falls_back_before() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let (db, names) = three_families();
        assert_eq!(names.len(), 3);
        let mut st = FontState::new();
        st.system.set(db);
        let cfg = FontsCfg::default();
        list_frame(&mut st, &ctx, &cfg, 0.0, &[]);
        let base = set_fonts_calls(&ctx);
        // scrolling: every frame shows other rows; nothing is read, set_fonts is not called
        let started = std::time::Instant::now();
        for i in 0..60 {
            let t = 1.0 + f64::from(i) * 0.02;
            list_frame(&mut st, &ctx, &cfg, t, &[names[i as usize % 3].clone()]);
            assert_eq!(st.list_look(&names[i as usize % 3]), ListLook::Fallback, "the row shows its name in Shale's font");
        }
        let scrolling = started.elapsed();
        assert_eq!((set_fonts_calls(&ctx), st.list.kept(), st.list.reading()), (base, 0, false), "a list that moves loads nothing");
        // at rest: the faces are read by the worker, then all go into egui at once
        let mut t = 3.0;
        while st.list.kept() < names.len() {
            t += 0.2;
            list_frame(&mut st, &ctx, &cfg, t, &names);
            std::thread::sleep(Duration::from_millis(5));
            assert!(t < 100.0, "the worker answers");
        }
        t += 0.2;
        let started = std::time::Instant::now();
        list_frame(&mut st, &ctx, &cfg, t, &names);
        let batch = started.elapsed();
        eprintln!("CFG-077: 60 frames of scrolling {scrolling:?}; the one frame that sets the batch of {} faces {batch:?}", names.len());
        assert_eq!(set_fonts_calls(&ctx), base + 1, "one set_fonts for the whole batch");
        // egui has the fonts from the next pass on
        for n in &names {
            assert_eq!(st.list_look(n), ListLook::Own { family: family_of(n), symbol: false });
        }
        // resting, scrolling back and forth over faces that are there: no more set_fonts
        for i in 0..40 {
            t += 0.05;
            list_frame(&mut st, &ctx, &cfg, t, &names[i % 3..]);
        }
        assert_eq!(set_fonts_calls(&ctx), base + 1);
        // the picker closes: the faces are dropped again, with one more set_fonts
        t += 0.05;
        list_frame(&mut st, &ctx, &cfg, t, &[]);
        t += 0.05;
        list_frame(&mut st, &ctx, &cfg, t, &[]);
        assert_eq!((set_fonts_calls(&ctx), st.list.kept(), st.list_look(&names[0])), (base + 2, 0, ListLook::Fallback));
        assert_eq!(st.installed, BTreeSet::new());
    }
}
