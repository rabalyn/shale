//! The key registry (KEY-001…009): every user-definable action with its id, label, scope and
//! default bindings, and the [`Keymap`] that holds the user's changes and finds the action a
//! key press belongs to.
//!
//! To add an action: one line in `actions!` below (variant, stable id, label, scope, defaults),
//! then `keymap.poll(input, scope)` in the code of that scope already returns it. A sequence is
//! a default like `"Space f f"`; a new scope is a variant of [`Scope`] plus its name.

use eframe::egui::{Event, InputState, Key, Modifiers};
use std::collections::{BTreeMap, HashMap};

/// Where a binding is active. A [`Scope::Global`] key wins over every other scope.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Scope {
    Global,
    FileList,
    SearchResults,
    MailView,
    /// Vim-lite normal mode (EDIT-015): sequences that start with Space.
    EditorNormal,
}

impl Scope {
    /// The scopes in the order the keybindings list shows them.
    pub const ALL: [Scope; 5] = [Scope::Global, Scope::FileList, Scope::SearchResults, Scope::MailView, Scope::EditorNormal];

    pub fn label(self) -> &'static str {
        match self {
            Scope::Global => "Everywhere",
            Scope::FileList => "File list (Ctrl+Shift+B)",
            Scope::SearchResults => "Search results",
            Scope::MailView => "Mail view",
            Scope::EditorNormal => "Vim-style editing, normal mode",
        }
    }

    /// Bare keys that are fixed in this scope and cannot be given to an action.
    fn reserved(self) -> &'static [Key] {
        match self {
            Scope::Global => &[],
            Scope::FileList => &[Key::Enter, Key::Escape],
            Scope::SearchResults => &[Key::Enter, Key::Escape, Key::Slash],
            Scope::MailView => &[Key::Enter, Key::Escape],
            // every key vim-lite uses in normal mode (EDIT-011…014), with or without Shift
            Scope::EditorNormal => &[
                Key::H,
                Key::J,
                Key::K,
                Key::L,
                Key::W,
                Key::B,
                Key::G,
                Key::N,
                Key::O,
                Key::I,
                Key::A,
                Key::X,
                Key::V,
                Key::P,
                Key::D,
                Key::Y,
                Key::Num0,
                Key::Num4,
                Key::Colon,
                Key::Slash,
                Key::Escape,
                Key::Enter,
                Key::Backspace,
                Key::ArrowLeft,
                Key::ArrowRight,
                Key::ArrowUp,
                Key::ArrowDown,
            ],
        }
    }

    /// Two scopes can see the same key press when they are the same or one is Global.
    fn overlaps(self, other: Scope) -> bool {
        self == other || self == Scope::Global || other == Scope::Global
    }
}

/// One key press: a key with modifiers (`ctrl` is Ctrl, or Cmd on a Mac).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Key,
}

impl Chord {
    pub fn new(mods: Modifiers, key: Key) -> Chord {
        Chord { ctrl: mods.command, alt: mods.alt, shift: mods.shift, key }
    }

    fn modifiers(self) -> Modifiers {
        let mut m = Modifiers::NONE;
        if self.ctrl {
            m |= Modifiers::COMMAND;
        }
        if self.alt {
            m |= Modifiers::ALT;
        }
        if self.shift {
            m |= Modifiers::SHIFT;
        }
        m
    }

    /// The stored text, e.g. `Ctrl+Shift+F`.
    pub fn to_text(self) -> String {
        let mut s = String::new();
        for (on, name) in [(self.ctrl, "Ctrl+"), (self.alt, "Alt+"), (self.shift, "Shift+")] {
            if on {
                s.push_str(name);
            }
        }
        s.push_str(self.key.name());
        s
    }

    /// What the user reads: like the text, with arrows and a lower-case letter for a bare key.
    pub fn display(self) -> String {
        let key = match self.key {
            Key::ArrowLeft => "←".to_string(),
            Key::ArrowRight => "→".to_string(),
            Key::ArrowUp => "↑".to_string(),
            Key::ArrowDown => "↓".to_string(),
            k if k.symbol_or_name().chars().count() == 1 && !self.ctrl && !self.alt && !self.shift => k.symbol_or_name().to_lowercase(),
            k => k.symbol_or_name().to_string(),
        };
        let mut s = String::new();
        for (on, name) in [(self.ctrl, "Ctrl+"), (self.alt, "Alt+"), (self.shift, "Shift+")] {
            if on {
                s.push_str(name);
            }
        }
        s + &key
    }

    pub fn parse(text: &str) -> Result<Chord, String> {
        let bad = || format!("“{text}” is not a key");
        let (mut ctrl, mut alt, mut shift) = (false, false, false);
        let mut rest = text;
        while let Some((m, tail)) = rest.split_once('+').filter(|(_, t)| !t.is_empty()) {
            match m {
                "Ctrl" => ctrl = true,
                "Alt" => alt = true,
                "Shift" => shift = true,
                _ => return Err(bad()),
            }
            rest = tail;
        }
        let key = Key::from_name(rest).ok_or_else(bad)?;
        Ok(Chord { ctrl, alt, shift, key })
    }
}

/// One or more chords typed one after the other.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Binding(pub Vec<Chord>);

impl Binding {
    /// `Ctrl+W`, `Space f f` (chords separated by spaces).
    pub fn parse(text: &str) -> Result<Binding, String> {
        let chords = text.split_whitespace().map(Chord::parse).collect::<Result<Vec<_>, _>>()?;
        if chords.is_empty() {
            return Err("an empty binding".into());
        }
        Ok(Binding(chords))
    }

    pub fn to_text(&self) -> String {
        self.0.iter().map(|c| c.to_text()).collect::<Vec<_>>().join(" ")
    }

    pub fn display(&self) -> String {
        self.0.iter().map(|c| c.display()).collect::<Vec<_>>().join(" ")
    }

    /// Equal, or one is the start of the other (then the shorter one would always win).
    fn clashes(&self, other: &Binding) -> bool {
        let n = self.0.len().min(other.0.len());
        self.0[..n] == other.0[..n]
    }
}

pub struct Def {
    pub action: Action,
    pub id: &'static str,
    pub label: &'static str,
    pub scope: Scope,
    pub defaults: &'static [&'static str],
}

macro_rules! actions {
    ($($variant:ident => $id:literal, $label:literal, $scope:ident, [$($default:literal),*];)*) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
        pub enum Action { $($variant),* }
        /// The registry: one row per user-definable action, in the order the list shows them.
        pub const DEFS: &[Def] = &[$(Def { action: Action::$variant, id: $id, label: $label, scope: Scope::$scope, defaults: &[$($default),*] }),*];
    };
}

actions! {
    FocusSidebar => "global.focus_sidebar", "Keyboard into the left sidebar", Global, ["Ctrl+H"];
    FocusEditor => "global.focus_editor", "Keyboard back into the editor", Global, ["Ctrl+L"];
    ShowFiles => "global.show_files", "File list with the keyboard in it", Global, ["Ctrl+Shift+B"];
    Search => "global.search", "Search all notes", Global, ["Ctrl+Shift+F"];
    MailView => "global.mail_view", "Mail view", Global, ["Ctrl+Shift+M"];
    QuickSwitcher => "global.quick_switcher", "Quick switcher", Global, ["Ctrl+O"];
    NewNote => "global.new_note", "New note", Global, ["Ctrl+N"];
    ToggleReading => "global.toggle_reading", "Edit / reading view", Global, ["Ctrl+E"];
    Back => "global.back", "Back", Global, ["Alt+Left"];
    Forward => "global.forward", "Forward", Global, ["Alt+Right"];
    CloseTab => "global.close_tab", "Close tab", Global, ["Ctrl+W"];
    Save => "global.save", "Save now", Global, ["Ctrl+S"];
    Settings => "global.settings", "Settings", Global, ["Ctrl+Comma"];
    DataUsage => "global.data_usage", "Data usage (what Shale keeps on disk, its caches)", Global, ["Ctrl+Shift+D"];
    PrevTab => "global.prev_tab", "Previous tab", Global, ["Ctrl+Alt+H"];
    NextTab => "global.next_tab", "Next tab", Global, ["Ctrl+Alt+L"];
    NormalAlternate => "normal.alternate_view", "Back to the previous view of this tab (again: forward)", EditorNormal, ["Space Tab"];
    NormalFindFile => "normal.find_file", "Find a file by name", EditorNormal, ["Space F F"];
    TreeNextFile => "files.next_file", "Next file", FileList, ["J", "Down"];
    TreePrevFile => "files.prev_file", "Previous file", FileList, ["K", "Up"];
    TreeNextFolder => "files.next_folder", "Next folder", FileList, ["Ctrl+J"];
    TreePrevFolder => "files.prev_folder", "Previous folder", FileList, ["Ctrl+K"];
    TreeToggleFolder => "files.toggle_folder", "Open or close the folder", FileList, ["Space"];
    HitNext => "search.next_hit", "Next hit", SearchResults, ["J", "Down"];
    HitPrev => "search.prev_hit", "Previous hit", SearchResults, ["K", "Up"];
    HitNextFile => "search.next_file", "First hit of the next file", SearchResults, ["Ctrl+J"];
    HitPrevFile => "search.prev_file", "First hit of the previous file", SearchResults, ["Ctrl+K"];
    // Mail view: the Shift keys come before the bare key of the same letter (egui lets a bare key match with Shift held)
    MailReplyAll => "mail.reply_all", "Reply all", MailView, ["Shift+R"];
    MailForward => "mail.forward", "Forward", MailView, ["Shift+F"];
    MailAttSave => "mail.attachment_save", "Save the selected attachment to the vault", MailView, ["Shift+O"];
    MailLoad => "mail.load", "Load the message (or the list of the folder) from the server", MailView, ["Shift+L"];
    MailNext => "mail.next", "Next folder, message or line of the pane with the keyboard", MailView, ["J", "Down"];
    MailPrev => "mail.prev", "Previous folder, message or line of the pane with the keyboard", MailView, ["K", "Up"];
    MailNextFolder => "mail.next_folder", "Next folder", MailView, ["Ctrl+J"];
    MailPrevFolder => "mail.prev_folder", "Previous folder", MailView, ["Ctrl+K"];
    MailMark => "mail.mark", "Mark the message for a selection", MailView, ["Space"];
    MailSearch => "mail.search", "Search mail", MailView, ["Slash"];
    MailSearchScope => "mail.search_scope", "Search all folders or this folder", MailView, ["T"];
    MailImport => "mail.import", "Import to vault", MailView, ["I"];
    MailRead => "mail.read", "Mark read or unread", MailView, ["U"];
    MailFlag => "mail.flag", "Set or clear the flag", MailView, ["S", "F"];
    MailDelete => "mail.delete", "Delete", MailView, ["D", "Delete"];
    MailMove => "mail.move", "Move to a folder", MailView, ["M"];
    MailCompose => "mail.compose", "Write a new mail", MailView, ["C"];
    MailReply => "mail.reply", "Reply", MailView, ["R"];
    MailSync => "mail.sync", "Sync, or cancel the sync", MailView, ["Ctrl+R"];
    MailAccounts => "mail.accounts", "Account settings", MailView, ["A"];
    MailUndoSend => "mail.undo_send", "Take back the mail waiting to be sent", MailView, ["Z"];
    MailAttNext => "mail.attachment_next", "Select the next attachment", MailView, ["CloseBracket"];
    MailAttOpen => "mail.attachment_open", "Open the selected attachment", MailView, ["O"];
}

impl Action {
    pub fn def(self) -> &'static Def {
        DEFS.iter().find(|d| d.action == self).expect("every action has a row in DEFS")
    }

    fn defaults(self) -> Vec<Binding> {
        self.def().defaults.iter().map(|d| Binding::parse(d).expect("the defaults in DEFS are valid")).collect()
    }
}

/// Ctrl chords of text editing, which a Global binding must not swallow.
const RESERVED_TEXT: [(bool, Key); 7] = [(false, Key::A), (false, Key::C), (false, Key::V), (false, Key::X), (false, Key::Z), (false, Key::Y), (true, Key::Z)];

/// Take this chord's key press out of the frame's input, if it was pressed with exactly these
/// modifiers (so Ctrl+Alt+L is not Ctrl+L). A bare key also takes the character it typed, so a
/// binding like `Space f f` never reaches a text box or vim as typing.
fn consume_exact(input: &mut InputState, c: Chord) -> bool {
    let want = c.modifiers();
    let mut found = false;
    input.events.retain(|e| {
        let hit = matches!(e, Event::Key { key, pressed: true, modifiers, .. } if *key == c.key && modifiers.matches_exact(want));
        found |= hit;
        !hit
    });
    if found && !c.ctrl && !c.alt {
        let typed = if c.key == Key::Space { " ".to_string() } else { c.key.symbol_or_name().to_string() };
        if let Some(i) = input.events.iter().position(|e| matches!(e, Event::Text(t) if t.eq_ignore_ascii_case(&typed))) {
            input.events.remove(i);
        }
    }
    found
}

#[derive(Default)]
pub struct Keymap {
    custom: BTreeMap<Action, Vec<Binding>>,
    /// The chords typed so far of an unfinished sequence, per scope.
    pending: HashMap<Scope, Vec<Chord>>,
    /// Set while the keybindings dialog records a key: nothing may trigger meanwhile.
    pub recording: bool,
}

impl Keymap {
    /// The keymap for the saved changes (`"id" -> ["binding", …]`). What is unknown, unreadable
    /// or in conflict is ignored (KEY-007).
    pub fn from_overrides(saved: &BTreeMap<String, Vec<String>>) -> Keymap {
        let mut k = Keymap::default();
        for d in DEFS {
            let Some(texts) = saved.get(d.id) else { continue };
            let parsed: Result<Vec<Binding>, String> = texts.iter().map(|t| Binding::parse(t)).collect();
            if let Ok(b) = parsed.and_then(|b| if b.is_empty() { Err(String::new()) } else { Ok(b) }) {
                k.custom.insert(d.action, b);
            }
        }
        // drop the changes that clash with another action, one by one
        for a in k.custom.keys().copied().collect::<Vec<_>>() {
            let bindings = k.bindings(a);
            if bindings.iter().any(|b| k.check(a, b).is_err()) {
                k.custom.remove(&a);
            }
        }
        k
    }

    /// The changes to store (KEY-007).
    pub fn overrides(&self) -> BTreeMap<String, Vec<String>> {
        self.custom.iter().map(|(a, b)| (a.def().id.to_string(), b.iter().map(Binding::to_text).collect())).collect()
    }

    pub fn bindings(&self, a: Action) -> Vec<Binding> {
        self.custom.get(&a).cloned().unwrap_or_else(|| a.defaults())
    }

    pub fn is_default(&self, a: Action) -> bool {
        !self.custom.contains_key(&a)
    }

    /// The bindings as the user reads them, e.g. `j / ↓`.
    pub fn display(&self, a: Action) -> String {
        self.bindings(a).iter().map(Binding::display).collect::<Vec<_>>().join(" / ")
    }

    /// Can `a` take `b`? The error is the message for the user (KEY-004, KEY-005).
    pub fn check(&self, a: Action, b: &Binding) -> Result<(), String> {
        let scope = a.def().scope;
        let first = b.0[0];
        if scope == Scope::Global && !first.ctrl && !first.alt {
            return Err(format!("{} needs Ctrl or Alt: a shortcut that works everywhere must not get in the way of typing", b.display()));
        }
        if b.0.len() == 1 && scope == Scope::Global && RESERVED_TEXT.iter().any(|(shift, key)| first.ctrl && !first.alt && first.shift == *shift && first.key == *key) {
            return Err(format!("{} is reserved for text editing", b.display()));
        }
        // a fixed key cannot start a binding (also not a sequence, which it would never complete)
        if !first.ctrl && !first.alt && scope.reserved().contains(&first.key) {
            return Err(format!("{} has a fixed meaning here", b.display()));
        }
        for d in DEFS.iter().filter(|d| d.action != a && d.scope.overlaps(scope)) {
            if self.bindings(d.action).iter().any(|o| o.clashes(b)) {
                return Err(format!("{} is already used by “{}”", b.display(), d.label));
            }
        }
        Ok(())
    }

    /// Give `a` this one binding (replacing its others); nothing changes when it is refused.
    pub fn set(&mut self, a: Action, b: Binding) -> Result<(), String> {
        self.check(a, &b)?;
        if a.defaults() == [b.clone()] {
            self.custom.remove(&a);
        } else {
            self.custom.insert(a, vec![b]);
        }
        Ok(())
    }

    /// Forget the chords of an unfinished sequence of this scope (its keys are not being read).
    pub fn cancel(&mut self, scope: Scope) {
        self.pending.remove(&scope);
    }

    pub fn reset(&mut self, a: Action) {
        self.custom.remove(&a);
    }

    pub fn reset_all(&mut self) {
        self.custom.clear();
    }

    /// The action of this scope whose binding was just completed by the keys pressed this frame
    /// (the keys are consumed). Starting a sequence returns `None` and remembers its chords; a
    /// key that continues no sequence forgets them (KEY-009).
    pub fn poll(&mut self, input: &mut InputState, scope: Scope) -> Option<Action> {
        if self.recording {
            return None;
        }
        let pending: Vec<Chord> = self.pending.get(&scope).cloned().unwrap_or_default();
        let depth = pending.len();
        let mut started = None;
        for d in DEFS.iter().filter(|d| d.scope == scope) {
            for b in self.bindings(d.action) {
                if b.0.len() <= depth || b.0[..depth] != pending[..] {
                    continue;
                }
                let next = b.0[depth];
                if consume_exact(input, next) {
                    if b.0.len() == depth + 1 {
                        self.pending.remove(&scope);
                        return Some(d.action);
                    }
                    started = Some(next);
                    break;
                }
            }
            if started.is_some() {
                break;
            }
        }
        match started {
            Some(c) => self.pending.entry(scope).or_default().push(c),
            None if depth > 0 && input.events.iter().any(|e| matches!(e, eframe::egui::Event::Key { pressed: true, .. })) => {
                self.pending.remove(&scope);
            }
            None => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, Event, RawInput};

    fn b(s: &str) -> Binding {
        Binding::parse(s).unwrap()
    }

    // spec: KEY-001
    #[test]
    fn every_default_parses_and_has_no_conflict() {
        let k = Keymap::default();
        let mut ids = std::collections::HashSet::new();
        for d in DEFS {
            assert!(ids.insert(d.id), "double id {}", d.id);
            assert!(!d.defaults.is_empty(), "{} has no default", d.id);
            for text in d.defaults {
                let bind = Binding::parse(text).unwrap_or_else(|e| panic!("{}: {e}", d.id));
                assert_eq!(bind.to_text(), *text, "defaults are written the way they are stored");
                assert_eq!(k.check(d.action, &bind), Ok(()), "{} {text}", d.id);
            }
        }
    }

    // spec: KEY-002
    #[test]
    fn the_definable_actions_are_the_listed_ones() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/spec/links-and-editor.md")).unwrap();
        let row = text.lines().find(|l| l.starts_with("| KEY-002 |")).unwrap();
        let k = Keymap::default();
        for d in DEFS.iter().filter(|d| d.scope == Scope::Global) {
            let shown = k.display(d.action);
            assert!(row.contains(&shown), "KEY-002 does not name {shown} ({})", d.id);
        }
        assert_eq!(DEFS.len(), 23 + 23 + 4);
    }

    // spec: KEY-003
    #[test]
    fn bindings_read_and_write_as_text() {
        assert_eq!(b("Ctrl+Shift+F").to_text(), "Ctrl+Shift+F");
        assert_eq!(b("Space f f").0.len(), 3);
        assert_eq!(b("Space F F").to_text(), "Space F F");
        assert_eq!(b("Alt+Left").display(), "Alt+←");
        assert_eq!(b("J").display(), "j");
        assert_eq!(b("Ctrl+J").display(), "Ctrl+J");
        assert_eq!(b("Ctrl+Comma").display(), "Ctrl+,");
        assert!(Binding::parse("").is_err() && Binding::parse("Ctrl+").is_err() && Binding::parse("Hyper+H").is_err());
        // every key writes to text it can be read back from
        for key in Key::ALL {
            let c = Chord { ctrl: true, alt: false, shift: true, key: *key };
            assert_eq!(Chord::parse(&c.to_text()), Ok(c), "{key:?}");
        }
    }

    // spec: KEY-004
    #[test]
    fn conflicts_are_found_in_a_scope_and_against_global_only() {
        let mut k = Keymap::default();
        // the message names the other action
        assert_eq!(k.check(Action::FocusEditor, &b("Ctrl+H")), Err("Ctrl+H is already used by “Keyboard into the left sidebar”".to_string()));
        // a Global key shadows the other scopes ...
        assert_eq!(k.check(Action::TreeNextFile, &b("Ctrl+H")), Err("Ctrl+H is already used by “Keyboard into the left sidebar”".to_string()));
        // ... and a scope key blocks a new Global one
        assert_eq!(k.check(Action::Back, &b("Ctrl+J")), Err("Ctrl+J is already used by “Next folder”".to_string()));
        // within a scope; the same key in unrelated scopes is fine
        assert_eq!(k.check(Action::TreePrevFile, &b("J")), Err("j is already used by “Next file”".to_string()));
        assert_eq!(k.check(Action::HitNext, &b("J")), Ok(()), "own old key");
        assert_eq!(k.check(Action::HitPrev, &b("Ctrl+J")), Err("Ctrl+J is already used by “First hit of the next file”".to_string()));
        k.set(Action::TreeNextFolder, b("Ctrl+Alt+J")).unwrap();
        assert_eq!(k.check(Action::HitPrev, &b("Ctrl+J")), Err("Ctrl+J is already used by “First hit of the next file”".to_string()));
        assert_eq!(k.check(Action::HitNextFile, &b("Ctrl+Alt+J")), Ok(()), "File list and Search results do not see each other");
        // a sequence clashes with its own start
        k.set(Action::TreeToggleFolder, b("Space f f")).unwrap();
        assert!(k.check(Action::TreePrevFile, &b("Space")).is_err());
        assert!(k.check(Action::TreePrevFile, &b("Space f g")).is_ok());
        assert!(k.check(Action::TreePrevFile, &b("Space f f g")).is_err());
    }

    // spec: KEY-005
    #[test]
    fn reserved_and_plain_global_keys_are_refused() {
        let k = Keymap::default();
        assert!(k.check(Action::Save, &b("G")).unwrap_err().contains("needs Ctrl or Alt"));
        assert!(k.check(Action::Save, &b("Space g")).is_err());
        assert!(k.check(Action::Save, &b("Ctrl+C")).unwrap_err().contains("reserved"));
        assert!(k.check(Action::Save, &b("Ctrl+Shift+Z")).unwrap_err().contains("reserved"));
        assert_eq!(k.check(Action::Save, &b("Ctrl+Shift+Y")), Ok(()));
        assert!(k.check(Action::TreeNextFile, &b("Enter")).unwrap_err().contains("fixed"));
        assert!(k.check(Action::HitNext, &b("Slash")).unwrap_err().contains("fixed"));
        assert_eq!(k.check(Action::TreeNextFile, &b("Slash")), Ok(()));
    }

    // spec: EDIT-015, KEY-005
    #[test]
    fn the_keys_of_vim_normal_mode_cannot_be_bound() {
        let k = Keymap::default();
        let a = Action::NormalAlternate;
        for text in ["H", "Shift+W", "G", "0", "4", "Slash", "Colon", "Escape", "Enter", "Backspace", "ArrowDown", "G X", "Shift+O Tab"] {
            assert!(k.check(a, &b(text)).unwrap_err().contains("fixed meaning"), "{text}");
        }
        // after the leader any key is free, and Ctrl chords are no vim keys
        assert_eq!(k.check(a, &b("Space G")), Ok(()));
        assert_eq!(k.check(a, &b("Space Tab")), Ok(()));
        assert_eq!(k.check(a, &b("Ctrl+J")), Ok(()));
        // the other scopes keep their own fixed keys
        assert_eq!(k.check(Action::TreeNextFile, &b("H")), Ok(()));
    }

    // spec: EDIT-015
    #[test]
    fn a_sequence_key_does_not_leave_its_typed_character() {
        let ctx = egui::Context::default();
        let mut k = Keymap::default();
        for (key, text) in [(Key::Space, " "), (Key::Tab, "")] {
            let mut events = vec![key_event(key, Modifiers::NONE)];
            events.push(Event::Text(text.to_string()));
            let (mut act, mut left) = (None, usize::MAX);
            let mut full = ctx.run_ui(RawInput { events, ..Default::default() }, |ui| {
                act = ui.input_mut(|i| k.poll(i, Scope::EditorNormal));
                left = ui.input(|i| i.events.iter().filter(|e| matches!(e, Event::Text(t) if !t.is_empty())).count());
            });
            full.textures_delta.clear();
            assert_eq!(left, 0, "{key:?} left its character for the text box");
            assert_eq!(act, (key == Key::Tab).then_some(Action::NormalAlternate));
        }
    }

    // spec: KEY-007
    #[test]
    fn overrides_round_trip_through_the_config_file() {
        let mut k = Keymap::default();
        assert!(k.overrides().is_empty(), "only changes are stored");
        k.set(Action::FocusSidebar, b("Ctrl+Alt+B")).unwrap();
        k.set(Action::TreeToggleFolder, b("Space f f")).unwrap();
        k.set(Action::Save, b("Ctrl+S")).unwrap();
        assert_eq!(k.overrides().len(), 2, "a binding put back to its default is no change");
        let mut cfg = crate::config::Config { keybindings: k.overrides(), ..Default::default() };
        let dir = std::env::temp_dir().join(format!("shale-keys-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.json");
        crate::config::write_private(&path, serde_json::to_string(&cfg).unwrap().as_bytes()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"global.focus_sidebar\":[\"Ctrl+Alt+B\"]") && text.contains("\"version\":1"), "{text}");
        let (loaded, problem) = crate::config::Config::load_from(&path);
        assert_eq!(problem, None);
        let k2 = Keymap::from_overrides(&loaded.keybindings);
        assert_eq!(k2.bindings(Action::FocusSidebar), vec![b("Ctrl+Alt+B")]);
        assert_eq!(k2.bindings(Action::TreeToggleFolder), vec![b("Space f f")]);
        assert_eq!(k2.bindings(Action::FocusEditor), Action::FocusEditor.defaults());
        // a file without the field means all defaults
        cfg.keybindings.clear();
        assert!(Keymap::from_overrides(&cfg.keybindings).overrides().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: KEY-007
    #[test]
    fn bad_overrides_in_the_config_are_ignored() {
        let saved: BTreeMap<String, Vec<String>> = [
            ("global.nonsense", vec!["Ctrl+Q"]),
            ("global.back", vec!["Ctrl+Hyper"]),
            ("global.forward", vec![]),
            // takes the key of "Keyboard into the left sidebar", which keeps it
            ("global.close_tab", vec!["Ctrl+H"]),
            ("global.new_note", vec!["Ctrl+Alt+N"]),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.into_iter().map(String::from).collect()))
        .collect();
        let k = Keymap::from_overrides(&saved);
        assert_eq!(k.overrides().keys().collect::<Vec<_>>(), ["global.new_note"]);
        assert!(k.is_default(Action::Back) && k.is_default(Action::Forward) && k.is_default(Action::CloseTab));
    }

    fn key_event(key: Key, mods: Modifiers) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: mods }
    }

    /// One frame with these events; what `poll` answered for the scope.
    fn frame(k: &mut Keymap, scope: Scope, events: Vec<Event>) -> Option<Action> {
        let ctx = egui::Context::default();
        let mut out = None;
        let mut full = ctx.run_ui(RawInput { events, ..Default::default() }, |ui| {
            out = ui.input_mut(|i| k.poll(i, scope));
        });
        full.textures_delta.clear();
        out
    }

    // spec: KEY-009, KEY-008
    #[test]
    fn a_sequence_fires_on_its_last_chord() {
        let mut k = Keymap::default();
        k.set(Action::TreeToggleFolder, b("Space f f")).unwrap();
        let none = Modifiers::NONE;
        let sc = Scope::FileList;
        assert_eq!(frame(&mut k, sc, vec![key_event(Key::Space, none)]), None, "the start of a sequence does nothing yet");
        assert_eq!(frame(&mut k, sc, vec![key_event(Key::F, none)]), None);
        assert_eq!(frame(&mut k, sc, vec![key_event(Key::F, none)]), Some(Action::TreeToggleFolder));
        // another key in between cancels it
        assert_eq!(frame(&mut k, sc, vec![key_event(Key::Space, none)]), None);
        assert_eq!(frame(&mut k, sc, vec![key_event(Key::G, none)]), None);
        assert_eq!(frame(&mut k, sc, vec![key_event(Key::F, none)]), None);
        assert_eq!(frame(&mut k, sc, vec![key_event(Key::F, none)]), None);
        // the old key is gone, a single chord still works, and scopes stay apart
        assert_eq!(frame(&mut k, sc, vec![key_event(Key::J, none)]), Some(Action::TreeNextFile));
        assert_eq!(frame(&mut k, Scope::SearchResults, vec![key_event(Key::Space, none)]), None);
        assert_eq!(frame(&mut k, Scope::Global, vec![key_event(Key::H, Modifiers::COMMAND)]), Some(Action::FocusSidebar));
        // nothing triggers while a key is being recorded
        k.recording = true;
        assert_eq!(frame(&mut k, Scope::Global, vec![key_event(Key::H, Modifiers::COMMAND)]), None);
    }

    // spec: NAV-006
    #[test]
    fn the_docs_list_every_default_binding() {
        let k = Keymap::default();
        for file in ["README.md", "docs/FEATURES.md"] {
            let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file)).unwrap();
            let table = text.split("\n## Keyboard\n").nth(1).unwrap_or_else(|| panic!("{file} has no Keyboard section")).split("\n## ").next().unwrap();
            for d in DEFS {
                for bind in k.bindings(d.action) {
                    let shown = bind.display();
                    // as a whole word: "j" must not count for the j of "jump"
                    let found = table.match_indices(&shown).any(|(i, m)| {
                        let before = table[..i].chars().next_back();
                        let after = table[i + m.len()..].chars().next();
                        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric());
                        !word(before) && !word(after)
                    });
                    assert!(found, "{file}: the Keyboard table does not list {shown} ({})", d.label);
                }
            }
        }
    }
}
