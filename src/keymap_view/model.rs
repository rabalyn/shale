//! The data of the keymap view (KEY-055): for a system and a layer of modifiers, which actions sit on
//! which key. No drawing here.

use super::tables::{self, Match, Reserved};
use crate::keys::{Action, Binding, Chord, Def, Keymap, Scope, DEFS};
use eframe::egui::Key;

/// The system the keyboard is shown for: its defaults (KEY-019) and its reserved keys differ.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum System {
    Linux,
    Windows,
    MacOs,
}

impl System {
    pub const ALL: [System; 3] = [System::Linux, System::Windows, System::MacOs];

    /// The system this program runs on.
    pub fn running() -> System {
        if cfg!(target_os = "macos") {
            System::MacOs
        } else if cfg!(target_os = "windows") {
            System::Windows
        } else {
            System::Linux
        }
    }

    pub fn is_mac(self) -> bool {
        self == System::MacOs
    }

    pub fn label(self) -> &'static str {
        match self {
            System::Linux => "Linux",
            System::Windows => "Windows",
            System::MacOs => "macOS",
        }
    }

    /// The name in files and screenshot names.
    pub fn id(self) -> &'static str {
        match self {
            System::Linux => "linux",
            System::Windows => "windows",
            System::MacOs => "macos",
        }
    }

    pub fn from_id(id: &str) -> Option<System> {
        System::ALL.into_iter().find(|s| s.id() == id)
    }

    /// The letter of the tables (`l`, `w`, `m`).
    fn flag(self) -> char {
        match self {
            System::Linux => 'l',
            System::Windows => 'w',
            System::MacOs => 'm',
        }
    }

    /// What the modifier is called on this system's keyboard.
    pub fn name_of(self, m: Mod) -> &'static str {
        match (m, self) {
            (Mod::Ctrl, System::MacOs) => "Cmd",
            (Mod::Ctrl, _) => "Ctrl",
            (Mod::Alt, System::MacOs) => "Option",
            (Mod::Alt, _) => "Alt",
            (Mod::Shift, _) => "Shift",
            (Mod::Super, System::MacOs) => "Control",
            (Mod::Super, System::Windows) => "Win",
            (Mod::Super, System::Linux) => "Super",
        }
    }
}

/// One of the four modifier keys. `Ctrl` is the key the registry calls Ctrl, which is Cmd on macOS
/// (`Chord::ctrl`); `Super` is the one the registry never binds: Win, Super, or Control on a Mac.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum Mod {
    Ctrl,
    Alt,
    Shift,
    Super,
}

impl Mod {
    pub const ALL: [Mod; 4] = [Mod::Ctrl, Mod::Alt, Mod::Shift, Mod::Super];

    fn from_name(name: &str) -> Option<Mod> {
        Some(match name {
            "Ctrl" => Mod::Ctrl,
            "Alt" => Mod::Alt,
            "Shift" => Mod::Shift,
            "Super" => Mod::Super,
            _ => return None,
        })
    }
}

/// The modifiers held with a key: a layer of the keyboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default, PartialOrd, Ord)]
pub struct Layer {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub sup: bool,
}

impl Layer {
    pub const NONE: Layer = Layer { ctrl: false, alt: false, shift: false, sup: false };
    pub const SHIFT: Layer = Layer { shift: true, ..Layer::NONE };
    pub const CTRL: Layer = Layer { ctrl: true, ..Layer::NONE };
    pub const ALT: Layer = Layer { alt: true, ..Layer::NONE };
    pub const CTRL_SHIFT: Layer = Layer { ctrl: true, shift: true, ..Layer::NONE };
    pub const ALT_SHIFT: Layer = Layer { alt: true, shift: true, ..Layer::NONE };
    pub const CTRL_ALT: Layer = Layer { ctrl: true, alt: true, ..Layer::NONE };

    /// The layers with a button of their own, in the order of the buttons.
    pub const PRESETS: [Layer; 7] = [Layer::NONE, Layer::SHIFT, Layer::CTRL, Layer::ALT, Layer::CTRL_SHIFT, Layer::ALT_SHIFT, Layer::CTRL_ALT];

    pub fn has(self, m: Mod) -> bool {
        match m {
            Mod::Ctrl => self.ctrl,
            Mod::Alt => self.alt,
            Mod::Shift => self.shift,
            Mod::Super => self.sup,
        }
    }

    /// The layer with `m` switched on or off.
    pub fn with(mut self, m: Mod, on: bool) -> Layer {
        match m {
            Mod::Ctrl => self.ctrl = on,
            Mod::Alt => self.alt = on,
            Mod::Shift => self.shift = on,
            Mod::Super => self.sup = on,
        }
        self
    }

    pub fn is_none(self) -> bool {
        self == Layer::NONE
    }

    /// Does this layer hold every modifier of `other`.
    fn holds(self, other: Layer) -> bool {
        Mod::ALL.into_iter().all(|m| !other.has(m) || self.has(m))
    }

    /// `Ctrl+Shift` (`Cmd+Shift` on macOS), `no modifier` for none.
    pub fn name(self, system: System) -> String {
        if self.is_none() {
            return "no modifier".to_string();
        }
        [Mod::Super, Mod::Ctrl, Mod::Alt, Mod::Shift].into_iter().filter(|m| self.has(*m)).map(|m| system.name_of(m)).collect::<Vec<_>>().join("+")
    }

    /// The name in file names and screenshot recipes: `none`, `shift`, `cmd-shift` (macOS), `ctrl-alt`.
    pub fn id(self, system: System) -> String {
        if self.is_none() {
            return "none".to_string();
        }
        self.name(system).to_lowercase().replace('+', "-")
    }

    pub fn from_id(system: System, id: &str) -> Option<Layer> {
        (0..16u8).map(|bits| Layer { ctrl: bits & 1 != 0, alt: bits & 2 != 0, shift: bits & 4 != 0, sup: bits & 8 != 0 }).find(|l| l.id(system) == id)
    }

    /// The layer the first chord of a binding is on.
    pub fn of_chord(c: Chord) -> Layer {
        Layer { ctrl: c.ctrl, alt: c.alt, shift: c.shift, sup: false }
    }

    /// `Ctrl+Shift+K` into the layer and the key; `Super+Alt` (modifiers only) has no key.
    pub fn parse(text: &str) -> Option<(Layer, Option<Key>)> {
        let mut layer = Layer::NONE;
        let mut tokens = text.split('+').peekable();
        while let Some(t) = tokens.next() {
            match Mod::from_name(t) {
                Some(m) => layer = layer.with(m, true),
                None if tokens.peek().is_none() => return Some((layer, Some(Key::from_name(t)?))),
                None => return None,
            }
        }
        Some((layer, None))
    }
}

/// What kind of action a key belongs to: the colour of its chip.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum Category {
    Moving,
    App,
    Format,
    Ribbon,
    Mail,
    Text,
    Vim,
    Draw,
}

impl Category {
    pub const ALL: [Category; 8] = [Category::Moving, Category::App, Category::Format, Category::Ribbon, Category::Mail, Category::Text, Category::Vim, Category::Draw];

    pub fn name(self) -> &'static str {
        match self {
            Category::Moving => "Moving around",
            Category::App => "Notes & app",
            Category::Format => "Formatting & folding",
            Category::Ribbon => "Ribbon & sidebars",
            Category::Mail => "Mail",
            Category::Text => "Text editing",
            Category::Vim => "Vim mode",
            Category::Draw => "Drawing",
        }
    }

    /// The colour as red, green, blue, for a light or a dark theme.
    pub fn colour(self, dark: bool) -> [u8; 3] {
        let (light, dark_) = match self {
            Category::Moving => ([0x2f, 0x7f, 0xd0], [0x5e, 0xa6, 0xf5]),
            Category::App => ([0x23, 0x96, 0x6a], [0x3f, 0xc4, 0x93]),
            Category::Format => ([0xe0, 0x78, 0x2a], [0xff, 0x9a, 0x4d]),
            Category::Ribbon => ([0xc3, 0x9a, 0x12], [0xe4, 0xc0, 0x43]),
            Category::Mail => ([0xcf, 0x4a, 0x76], [0xf0, 0x6f, 0x9b]),
            Category::Text => ([0x6f, 0x7d, 0x8e], [0x96, 0xa3, 0xb3]),
            Category::Vim => ([0x12, 0x9c, 0x9c], [0x35, 0xc9, 0xc9]),
            Category::Draw => ([0x84, 0x59, 0xcf], [0xa9, 0x88, 0xf0]),
        };
        if dark {
            dark_
        } else {
            light
        }
    }

    /// The category of a definable action, by its id.
    pub fn of_def(def: &Def) -> Category {
        let id = def.id;
        if id.starts_with("note.") {
            Category::Format
        } else if id.starts_with("mail.") {
            Category::Mail
        } else if id.starts_with("global.ribbon_") || id.starts_with("global.sidebar_") {
            Category::Ribbon
        } else if id.starts_with("files.") || id.starts_with("search.") || id.starts_with("list.") || id == "normal.alternate_view" || is_moving_global(id) {
            Category::Moving
        } else {
            Category::App
        }
    }
}

/// The Global actions that move the keyboard or the view around: focus, history, tabs, panes, the thread.
fn is_moving_global(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("global.") else { return false };
    ["focus_", "back", "forward", "prev_tab", "next_tab", "split_right", "next_pane", "prev_pane", "close_pane", "alternate_view", "thread_"].iter().any(|p| rest.starts_with(p))
}

/// Where a key works: the scopes of the registry, and those of the fixed keys.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum Where {
    Global,
    NoteView,
    Editor,
    Text,
    MailView,
    FileList,
    SearchResults,
    ListWindow,
    MainViews,
    EditorNormal,
    Drawing,
    Dialogs,
}

impl Where {
    pub const ALL: [Where; 12] = [Where::Global, Where::NoteView, Where::Editor, Where::Text, Where::MailView, Where::FileList, Where::SearchResults, Where::ListWindow, Where::MainViews, Where::EditorNormal, Where::Drawing, Where::Dialogs];

    pub fn label(self) -> &'static str {
        match self {
            Where::Global => "Everywhere",
            Where::NoteView => "Note",
            Where::Editor => "Editor",
            Where::Text => "Text fields",
            Where::MailView => "Mail",
            Where::FileList => "File list",
            Where::SearchResults => "Search results",
            Where::ListWindow => "Pickers & lists",
            Where::MainViews => "Main views",
            Where::EditorNormal => "Vim normal",
            Where::Drawing => "Drawing",
            Where::Dialogs => "Dialogs",
        }
    }
}

impl From<Scope> for Where {
    fn from(s: Scope) -> Where {
        match s {
            Scope::Global => Where::Global,
            Scope::FileList => Where::FileList,
            Scope::SearchResults => Where::SearchResults,
            Scope::ListWindow => Where::ListWindow,
            Scope::MailView => Where::MailView,
            Scope::MainViews => Where::MainViews,
            Scope::NoteView => Where::NoteView,
            Scope::EditorNormal => Where::EditorNormal,
        }
    }
}

/// Where an entry comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A definable action on its default key.
    Default,
    /// A definable action on a key the user chose.
    Changed,
    /// A key of the program that cannot be changed.
    Fixed,
}

/// One action on one key of one layer.
#[derive(Clone, Debug)]
pub struct Entry {
    pub layer: Layer,
    pub key: Key,
    pub label: &'static str,
    /// The short name for a chip.
    pub short: String,
    pub scope: Where,
    pub cat: Category,
    pub kind: Kind,
    /// The action, for a definable one (it has a Change button).
    pub action: Option<Action>,
    /// The binding as it is read on this system: `Ctrl+Shift+K`, `Cmd+Shift+K`, `Space f f`.
    pub text: String,
    /// The binding is a sequence: the entry sits on its first key.
    pub sequence: bool,
    pub note: &'static str,
}

/// Which entries the keyboard shows: the legend and the scope buttons switch kinds and places off.
#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub hidden_cats: Vec<Category>,
    pub hidden_scopes: Vec<Where>,
}

impl Filter {
    pub fn shows(&self, e: &Entry) -> bool {
        !self.hidden_cats.contains(&e.cat) && !self.hidden_scopes.contains(&e.scope)
    }
}

/// What is on one key of one layer, and on the other layers.
pub struct OnLayer<'a> {
    pub layer: Layer,
    pub entries: Vec<&'a Entry>,
    pub reserved: Option<&'static str>,
}

/// All entries for one system.
pub struct Model {
    pub system: System,
    entries: Vec<Entry>,
}

impl Model {
    /// The model for `system` from the live keymap: the user's changes are in it, the defaults are
    /// those of the system (KEY-019), so another system than the running one gets the changes on its own
    /// defaults.
    pub fn new(live: &Keymap, system: System) -> Model {
        let other;
        let km = if live.is_mac() == system.is_mac() {
            live
        } else {
            other = Keymap::from_overrides_for(&live.overrides(), system.is_mac());
            &other
        };
        let mut entries = vec![];
        for d in DEFS {
            let kind = if km.is_default(d.action) { Kind::Default } else { Kind::Changed };
            for b in km.bindings(d.action) {
                let first = b.0[0];
                entries.push(Entry {
                    layer: Layer::of_chord(first),
                    key: first.key,
                    label: d.label,
                    short: short_of(d),
                    scope: d.scope.into(),
                    cat: Category::of_def(d),
                    kind,
                    action: Some(d.action),
                    text: binding_text(system, &b),
                    sequence: b.0.len() > 1,
                    note: "",
                });
            }
        }
        for b in tables::BUILTIN.iter().filter(|b| b.systems.contains(system.flag())) {
            let Some((layer, Some(key))) = Layer::parse(b.keys) else { continue };
            entries.push(Entry { layer, key, label: b.label, short: b.label.to_string(), scope: b.scope, cat: b.cat, kind: Kind::Fixed, action: None, text: chord_text(system, layer, key), sequence: false, note: b.note });
        }
        // the keys of the user first, the fixed ones last, each in the order of the scopes
        entries.sort_by_key(|e| (e.kind == Kind::Fixed, e.scope));
        Model { system, entries }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entries on this key of this layer, in the order the key shows them.
    pub fn at(&self, layer: Layer, key: Key) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.layer == layer && e.key == key).collect()
    }

    /// What the system keeps this chord for, if it does.
    pub fn reserved(&self, layer: Layer, key: Key) -> Option<&'static str> {
        let table: &[Reserved] = match self.system {
            System::Linux => tables::RESERVED_LINUX,
            System::Windows => tables::RESERVED_WINDOWS,
            System::MacOs => tables::RESERVED_MACOS,
        };
        table.iter().find_map(|r| {
            let (mods, exact) = Layer::parse(r.keys)?;
            match r.kind {
                Match::Exact => (mods == layer && exact == Some(key)).then_some(r.label),
                Match::AnyKeyWith => layer.holds(mods).then_some(r.label),
                Match::AnyCharWith => (layer.holds(mods) && types_a_character(key)).then_some(r.label),
            }
        })
    }

    /// Every layer this key has an entry or a reserved chord on, fewest modifiers first.
    pub fn layers_of(&self, key: Key) -> Vec<OnLayer<'_>> {
        let mut layers: Vec<Layer> = self.entries.iter().filter(|e| e.key == key).map(|e| e.layer).collect();
        let table: &[Reserved] = match self.system {
            System::Linux => tables::RESERVED_LINUX,
            System::Windows => tables::RESERVED_WINDOWS,
            System::MacOs => tables::RESERVED_MACOS,
        };
        for r in table.iter().filter(|r| r.kind == Match::Exact) {
            if let Some((l, Some(k))) = Layer::parse(r.keys) {
                if k == key {
                    layers.push(l);
                }
            }
        }
        layers.sort_by_key(|l| (Mod::ALL.iter().filter(|m| l.has(**m)).count(), *l));
        layers.dedup();
        layers.into_iter().map(|layer| OnLayer { layer, entries: self.at(layer, key), reserved: self.reserved(layer, key) }).collect()
    }

    /// The sequences (several keys in a row), for the list under the keyboard.
    pub fn sequences(&self) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.sequence).collect()
    }
}

/// A key that types a character: letters, digits and punctuation.
fn types_a_character(key: Key) -> bool {
    let name = key.name();
    name.len() == 1 || matches!(key, Key::Comma | Key::Period | Key::Minus | Key::Equals | Key::Plus | Key::Slash | Key::Backslash | Key::OpenBracket | Key::CloseBracket | Key::Semicolon | Key::Quote | Key::Backtick)
}

/// The short name of an action for its chip.
pub fn short_of(def: &Def) -> String {
    if let Some((_, short)) = tables::SHORT.iter().find(|(id, _)| *id == def.id) {
        return short.to_string();
    }
    if let Some(n) = def.id.strip_prefix("global.ribbon_") {
        return format!("Ribbon {n}");
    }
    let label = def.label.strip_prefix("Note: ").or_else(|| def.label.strip_prefix("Open mail: ")).unwrap_or(def.label);
    let label = label.split(" button").next().unwrap_or(label);
    label.split(" (").next().unwrap_or(label).to_string()
}

/// What the key is called on a keycap.
pub fn key_text(key: Key) -> String {
    match key {
        Key::ArrowLeft => "←".to_string(),
        Key::ArrowRight => "→".to_string(),
        Key::ArrowUp => "↑".to_string(),
        Key::ArrowDown => "↓".to_string(),
        Key::Escape => "Esc".to_string(),
        k if k.symbol_or_name().chars().count() == 1 => k.symbol_or_name().to_uppercase(),
        k => k.symbol_or_name().to_string(),
    }
}

/// `Ctrl+Shift+K` as this system names the keys.
pub fn chord_text(system: System, layer: Layer, key: Key) -> String {
    let mut parts: Vec<String> = [Mod::Super, Mod::Ctrl, Mod::Alt, Mod::Shift].into_iter().filter(|m| layer.has(*m)).map(|m| system.name_of(m).to_string()).collect();
    parts.push(key_text(key));
    parts.join("+")
}

/// A binding as this system names the keys: the first chord with its modifiers, the others as they
/// are typed (`Space f f`).
pub fn binding_text(system: System, b: &Binding) -> String {
    let first = b.0[0];
    let mut s = chord_text(system, Layer::of_chord(first), first.key);
    for c in &b.0[1..] {
        s.push(' ');
        if c.ctrl || c.alt || c.shift {
            s.push_str(&chord_text(system, Layer::of_chord(*c), c.key));
        } else if c.key.symbol_or_name().chars().count() == 1 {
            s.push_str(&c.key.symbol_or_name().to_lowercase());
        } else {
            s.push_str(&key_text(c.key));
        }
    }
    s
}
