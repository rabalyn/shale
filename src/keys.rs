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
    /// The buttons of a note (KEY-014): active while the focused pane shows a note, in the editor and in the reading view.
    NoteView,
    /// Vim-lite normal mode (EDIT-015): sequences that start with Space.
    EditorNormal,
}

impl Scope {
    /// The scopes in the order the keybindings list shows them.
    pub const ALL: [Scope; 6] = [Scope::Global, Scope::FileList, Scope::SearchResults, Scope::MailView, Scope::NoteView, Scope::EditorNormal];

    pub fn label(self) -> &'static str {
        match self {
            Scope::Global => "Everywhere",
            Scope::FileList => "File list (Ctrl+Shift+B)",
            Scope::SearchResults => "Search results",
            Scope::MailView => "Mail view",
            Scope::NoteView => "Note view",
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
            Scope::NoteView => &[],
            // every key vim-lite uses in normal mode (EDIT-011…014), with or without Shift; but see
            // `Keymap::check`: Shift+K is no vim key (the link preview has it, EDIT-033)
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
                Key::U,
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

    /// Two scopes can see the same key press when they are the same, one is Global, or they are
    /// the note view and vim-style normal mode (the editor shows a note, KEY-014). Symmetric.
    fn overlaps(self, other: Scope) -> bool {
        self == other || self == Scope::Global || other == Scope::Global || matches!((self, other), (Scope::NoteView, Scope::EditorNormal) | (Scope::EditorNormal, Scope::NoteView))
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
    FocusSidebar => "global.focus_sidebar", "Keyboard one place to the left (pane, then left sidebar)", Global, ["Ctrl+H"];
    FocusEditor => "global.focus_editor", "Keyboard one place to the right (sidebar, then panes)", Global, ["Ctrl+L"];
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
    UserGuide => "global.user_guide", "Open the user guide", Global, ["F1"];
    PrevTab => "global.prev_tab", "Previous tab", Global, ["Ctrl+PageUp"];
    NextTab => "global.next_tab", "Next tab", Global, ["Ctrl+PageDown"];
    SplitRight => "global.split_right", "Split right: open this tab in a new pane to the right", Global, ["Ctrl+Backslash"];
    NextPane => "global.next_pane", "Focus the next pane", Global, ["F6"];
    PrevPane => "global.prev_pane", "Focus the previous pane", Global, ["Shift+F6"];
    ClosePane => "global.close_pane", "Close the pane (with its tabs)", Global, ["Ctrl+Shift+W"];
    AlternateView => "global.alternate_view", "Alternate view: swap with the previous view of this tab (again: back)", Global, ["Ctrl+6"];
    // The ribbon (KEY-015): its first ten shown items by position, and the sidebar toggles; no default on macOS
    RibbonSlot1 => "global.ribbon_1", "Ribbon: 1st item", Global, ["Alt+1"];
    RibbonSlot2 => "global.ribbon_2", "Ribbon: 2nd item", Global, ["Alt+2"];
    RibbonSlot3 => "global.ribbon_3", "Ribbon: 3rd item", Global, ["Alt+3"];
    RibbonSlot4 => "global.ribbon_4", "Ribbon: 4th item", Global, ["Alt+4"];
    RibbonSlot5 => "global.ribbon_5", "Ribbon: 5th item", Global, ["Alt+5"];
    RibbonSlot6 => "global.ribbon_6", "Ribbon: 6th item", Global, ["Alt+6"];
    RibbonSlot7 => "global.ribbon_7", "Ribbon: 7th item", Global, ["Alt+7"];
    RibbonSlot8 => "global.ribbon_8", "Ribbon: 8th item", Global, ["Alt+8"];
    RibbonSlot9 => "global.ribbon_9", "Ribbon: 9th item", Global, ["Alt+9"];
    RibbonSlot10 => "global.ribbon_10", "Ribbon: 10th item", Global, ["Alt+0"];
    SidebarLeft => "global.sidebar_left", "Show or hide the left sidebar", Global, ["Alt+Comma"];
    SidebarRight => "global.sidebar_right", "Show or hide the right sidebar", Global, ["Alt+Period"];
    // The buttons of a note (KEY-015): the formatting toolbar, the view header and the tab bar; no default on macOS
    NoteBold => "note.bold", "Note: Bold button", NoteView, ["Alt+B"];
    NoteItalic => "note.italic", "Note: Italic button", NoteView, ["Alt+I"];
    NoteStrike => "note.strike", "Note: Strikethrough button", NoteView, ["Alt+S"];
    NoteHighlight => "note.highlight", "Note: Highlight button", NoteView, ["Alt+H"];
    NoteCode => "note.code", "Note: Inline code button", NoteView, ["Alt+C"];
    NoteHighlightColour => "note.highlight_colour", "Note: Highlight colour button (opens its palette)", NoteView, ["Alt+Y"];
    NoteFontColour => "note.font_colour", "Note: Font colour button (opens its palette)", NoteView, ["Alt+F"];
    NoteHeading => "note.heading", "Note: Heading level button (opens its menu)", NoteView, ["Alt+E"];
    NoteBullet => "note.bullet", "Note: Bullet list button", NoteView, ["Alt+U"];
    NoteNumbered => "note.numbered", "Note: Numbered list button", NoteView, ["Alt+O"];
    NoteTask => "note.task", "Note: Task button", NoteView, ["Alt+T"];
    NoteQuote => "note.quote", "Note: Quote button", NoteView, ["Alt+Q"];
    NoteCodeBlock => "note.code_block", "Note: Code block button", NoteView, ["Alt+K"];
    NoteRule => "note.rule", "Note: Horizontal rule button", NoteView, ["Alt+R"];
    NotePageBreak => "note.page_break", "Note: Page break button", NoteView, ["Alt+J"];
    NoteTable => "note.table", "Note: Table button", NoteView, ["Alt+A"];
    NoteWikiLink => "note.wiki_link", "Note: Link to a note button", NoteView, ["Alt+L"];
    NoteWebLink => "note.web_link", "Note: Web link button", NoteView, ["Alt+W"];
    NoteEmbed => "note.embed", "Note: Embed button (opens the file picker)", NoteView, ["Alt+M"];
    NoteClear => "note.clear", "Note: Clear formatting button", NoteView, ["Alt+X"];
    NoteReading => "note.reading", "Note: Edit / reading view button", NoteView, ["Alt+V"];
    NotePreview => "note.preview", "Note: Markdown preview button", NoteView, ["Alt+P"];
    NoteMenu => "note.menu", "Note: ⋯ menu button (opens its menu)", NoteView, ["Alt+D"];
    NoteNewTab => "note.new_tab", "Note: New tab button", NoteView, ["Alt+N"];
    // Folding (KEY-030…KEY-034): bound with Ctrl and Shift, so macOS has the same keys
    NoteFold => "note.fold", "Fold the block at the cursor (again: the block around it)", NoteView, ["Ctrl+Shift+K"];
    NoteUnfold => "note.unfold", "Unfold the block at the cursor", NoteView, ["Ctrl+Shift+J"];
    NoteFoldAll => "note.fold_all", "Fold every block of the note", NoteView, ["Ctrl+Shift+O"];
    NoteUnfoldAll => "note.unfold_all", "Unfold every block of the note", NoteView, ["Ctrl+Shift+I"];
    // KEY-035…KEY-037
    NoteCopyCode => "note.copy_code", "Copy the code of the code block at the cursor", NoteView, ["Ctrl+Shift+C"];
    NormalAlternate => "normal.alternate_view", "Back to the previous view of this tab (again: forward)", EditorNormal, ["Space Tab"];
    NormalFindFile => "normal.find_file", "Find a file by name", EditorNormal, ["Space F F"];
    NormalSearchAll => "normal.search_all", "Search notes, mail and posts", EditorNormal, ["Space F G"];
    NormalLinkPreview => "normal.link_preview", "Preview the link under the cursor (again: into the preview)", EditorNormal, ["Shift+K"];
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
    MailRetry => "mail.retry", "Retry a mail of the outbox that was not sent (or send it again)", MailView, ["Shift+S"];
    MailUnsent => "mail.unsent", "Show or hide the sent mails that are only on this computer", MailView, ["Shift+U"];
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
    MailEdit => "mail.edit", "Edit the draft, or take a mail of the outbox back into the editor", MailView, ["E"];
    // The Alt shortcuts of the buttons of the open mail (MAIL-229): the letter is underlined on the button while Alt is held
    MailBtnImport => "mail.button_import", "Open mail: Import to vault button", MailView, ["Alt+I"];
    MailBtnClose => "mail.button_close", "Open mail: Close button", MailView, ["Alt+C"];
    MailBtnReply => "mail.button_reply", "Open mail: Reply button", MailView, ["Alt+R"];
    MailBtnReplyAll => "mail.button_reply_all", "Open mail: Reply all button", MailView, ["Alt+A"];
    MailBtnForward => "mail.button_forward", "Open mail: Forward button", MailView, ["Alt+F"];
    MailBtnRead => "mail.button_read", "Open mail: Mark read / unread button", MailView, ["Alt+U"];
    MailBtnFlag => "mail.button_flag", "Open mail: Flag / Unflag button", MailView, ["Alt+G"];
    MailBtnMove => "mail.button_move", "Open mail: Move button", MailView, ["Alt+M"];
    MailBtnDelete => "mail.button_delete", "Open mail: Delete button", MailView, ["Alt+D"];
    MailMenu => "mail.menu", "Open the menu of the message or draft under the cursor", MailView, ["Menu", "Shift+F10"];
}

impl Action {
    pub fn def(self) -> &'static Def {
        DEFS.iter().find(|d| d.action == self).expect("every action has a row in DEFS")
    }

    /// The default keys on Linux and Windows, or on macOS when `mac`.
    fn defaults_for(self, mac: bool) -> Vec<Binding> {
        if mac && self.is_alt_key() {
            return vec![];
        }
        let texts = if mac { self.mac_defaults().unwrap_or(self.def().defaults) } else { self.def().defaults };
        texts.iter().map(|d| Binding::parse(d).expect("the defaults in DEFS are valid")).collect()
    }

    /// The defaults of macOS where they differ from the table, because the app menu or the system
    /// keeps the key of Linux and Windows (KEY-019): Cmd+H hides the app, Option+arrows move in text.
    fn mac_defaults(self) -> Option<&'static [&'static str]> {
        Some(match self {
            Action::FocusSidebar => &["Ctrl+Shift+H"],
            Action::FocusEditor => &["Ctrl+Shift+L"],
            Action::Back => &["Ctrl+OpenBracket"],
            Action::Forward => &["Ctrl+CloseBracket"],
            Action::PrevTab => &["Ctrl+Alt+Left"],
            Action::NextTab => &["Ctrl+Alt+Right"],
            _ => return None,
        })
    }

    /// The Alt keys of the buttons of a note, of the open mail (MAIL-229) and of the ribbon (KEY-015): on macOS, where Option+letter
    /// types characters, they have no default key.
    pub fn is_alt_key(self) -> bool {
        let d = self.def();
        (d.scope == Scope::NoteView && d.defaults.iter().all(|k| k.starts_with("Alt+"))) || d.id.starts_with("global.ribbon_") || d.id.starts_with("global.sidebar_") || d.id.starts_with("mail.button_")
    }

    /// The place (0 to 9) of the ribbon item a `RibbonSlot` action opens.
    pub fn ribbon_slot(self) -> Option<usize> {
        RIBBON_SLOTS.iter().position(|&a| a == self)
    }
}

/// The actions of the first ten ribbon items, in order (KEY-015).
pub const RIBBON_SLOTS: [Action; 10] =
    [Action::RibbonSlot1, Action::RibbonSlot2, Action::RibbonSlot3, Action::RibbonSlot4, Action::RibbonSlot5, Action::RibbonSlot6, Action::RibbonSlot7, Action::RibbonSlot8, Action::RibbonSlot9, Action::RibbonSlot10];

/// Ctrl chords of text editing, which a Global binding must not swallow.
const RESERVED_TEXT: [(bool, Key); 7] = [(false, Key::A), (false, Key::C), (false, Key::V), (false, Key::X), (false, Key::Z), (false, Key::Y), (true, Key::Z)];

/// Take this chord's key press out of the frame's input, if it was pressed with exactly these
/// modifiers (so Ctrl+Alt+L is not Ctrl+L). A bare key, or an Alt key (without Ctrl), also takes the
/// character it typed, so a binding like `Space f f` or `Alt+B` never reaches a text box or vim as typing.
fn consume_exact(input: &mut InputState, c: Chord) -> bool {
    let want = c.modifiers();
    let mut found = false;
    input.events.retain(|e| {
        let hit = matches!(e, Event::Key { key, pressed: true, modifiers, .. } if *key == c.key && modifiers.matches_exact(want));
        found |= hit;
        !hit
    });
    if found && !c.ctrl {
        let typed = if c.key == Key::Space { " ".to_string() } else { c.key.symbol_or_name().to_string() };
        if let Some(i) = input.events.iter().position(|e| matches!(e, Event::Text(t) if t.eq_ignore_ascii_case(&typed))) {
            input.events.remove(i);
        }
    }
    found
}

/// How long an unfinished sequence waits for its next key (KEY-009).
pub const SEQUENCE_TIMEOUT: f64 = 1.5;

pub struct Keymap {
    custom: BTreeMap<Action, Vec<Binding>>,
    /// Whose defaults the keymap starts from: macOS has none for the Alt keys of the buttons (KEY-015).
    mac: bool,
    /// The chords typed so far of an unfinished sequence, per scope, and the time (egui seconds)
    /// of the last one (KEY-009).
    pending: HashMap<Scope, (Vec<Chord>, f64)>,
    /// Set while the keybindings dialog records a key: nothing may trigger meanwhile.
    pub recording: bool,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::for_os(cfg!(target_os = "macos"))
    }
}

impl Keymap {
    /// The keymap with the defaults of macOS (`mac`) or of Linux and Windows, and no changes.
    pub fn for_os(mac: bool) -> Keymap {
        Keymap { custom: BTreeMap::new(), mac, pending: HashMap::new(), recording: false }
    }

    /// The keymap for the saved changes (`"id" -> ["binding", …]`). What is unknown, unreadable
    /// or in conflict is ignored (KEY-007).
    pub fn from_overrides(saved: &BTreeMap<String, Vec<String>>) -> Keymap {
        Keymap::from_overrides_for(saved, cfg!(target_os = "macos"))
    }

    /// `from_overrides` on the given system (the defaults differ, KEY-015).
    pub fn from_overrides_for(saved: &BTreeMap<String, Vec<String>>, mac: bool) -> Keymap {
        let mut k = Keymap::for_os(mac);
        for d in DEFS {
            let Some(texts) = saved.get(d.id) else { continue };
            // an empty list is an action the user cleared: it stays unbound
            if let Ok(b) = texts.iter().map(|t| Binding::parse(t)).collect::<Result<Vec<Binding>, String>>() {
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
        self.custom.get(&a).cloned().unwrap_or_else(|| a.defaults_for(self.mac))
    }

    /// The single-chord Alt key of `a` (Alt without Ctrl or Shift), the one the badges and underlines show
    /// while Alt is held (KEY-016, MAIL-229); `None` when it has none.
    pub fn alt_chord(&self, a: Action) -> Option<Chord> {
        self.bindings(a).into_iter().find(|b| b.0.len() == 1 && b.0[0].alt && !b.0[0].ctrl && !b.0[0].shift).map(|b| b.0[0])
    }

    /// The text of the key of `a` for a badge: `B`, `1`, `,` (KEY-016); `None` without an Alt key.
    pub fn alt_badge(&self, a: Action) -> Option<String> {
        self.alt_chord(a).map(|c| c.key.symbol_or_name().to_uppercase())
    }

    pub fn is_default(&self, a: Action) -> bool {
        !self.custom.contains_key(&a)
    }

    /// The bindings as the user reads them, e.g. `j / ↓`; `not set` for a cleared action.
    pub fn display(&self, a: Action) -> String {
        let b = self.bindings(a);
        if b.is_empty() {
            return "not set".to_string();
        }
        b.iter().map(Binding::display).collect::<Vec<_>>().join(" / ")
    }

    /// The key of `a` as a hint on a button or menu entry (MAIL-072): the bindings as `display` shows
    /// them, `None` for an action that is not bound.
    pub fn hint(&self, a: Action) -> Option<String> {
        (!self.bindings(a).is_empty()).then(|| self.display(a))
    }

    /// Can `a` take `b`? The error is the message for the user (KEY-004, KEY-005).
    pub fn check(&self, a: Action, b: &Binding) -> Result<(), String> {
        self.check_rules(a, b)?;
        match self.clashes_of(a, b).first() {
            Some(other) => Err(format!("{} is already used by “{}”", b.display(), other.def().label)),
            None => Ok(()),
        }
    }

    /// The rules for a binding of `a` that no overwrite can lift: Ctrl or Alt, reserved keys, fixed
    /// keys (KEY-005). The error is the message for the user.
    pub fn check_rules(&self, a: Action, b: &Binding) -> Result<(), String> {
        let scope = a.def().scope;
        let first = b.0[0];
        let function_key = first.key.name().strip_prefix('F').is_some_and(|n| n.parse::<u8>().is_ok());
        if matches!(scope, Scope::Global | Scope::NoteView) && !first.ctrl && !first.alt && !function_key {
            return Err(format!("{} needs Ctrl or Alt: a shortcut that works everywhere must not get in the way of typing", b.display()));
        }
        if b.0.len() == 1 && matches!(scope, Scope::Global | Scope::NoteView) && RESERVED_TEXT.iter().any(|(shift, key)| first.ctrl && !first.alt && first.shift == *shift && first.key == *key) {
            return Err(format!("{} is reserved for text editing", b.display()));
        }
        // a fixed key cannot start a binding (also not a sequence, which it would never complete)
        // Ctrl+R redoes in vim normal mode (EDIT-021)
        // the editor moves rows with Alt+Up / Alt+Down (EDIT-026 lists, KEY-014)
        let moves_rows = scope == Scope::NoteView && first.alt && !first.ctrl && matches!(first.key, Key::ArrowUp | Key::ArrowDown);
        let vim_redo = scope == Scope::EditorNormal && first.ctrl && !first.alt && !first.shift && first.key == Key::R;
        // vim-lite has a plain k but no K (EDIT-033)
        let shift_k_is_free = scope == Scope::EditorNormal && first.shift && first.key == Key::K;
        if (!first.ctrl && !first.alt && scope.reserved().contains(&first.key) && !shift_k_is_free) || vim_redo || moves_rows {
            return Err(format!("{} has a fixed meaning here", b.display()));
        }
        Ok(())
    }

    /// The other actions, in the order of `DEFS`, that `b` would clash with if `a` took it: a binding
    /// equal to it or the start of it (or the other way round) in the same or an overlapping scope
    /// (KEY-004, KEY-017, KEY-018).
    pub fn clashes_of(&self, a: Action, b: &Binding) -> Vec<Action> {
        let scope = a.def().scope;
        DEFS.iter().filter(|d| d.action != a && d.scope.overlaps(scope) && self.bindings(d.action).iter().any(|o| o.clashes(b))).map(|d| d.action).collect()
    }

    /// Give `a` the binding `b` (in addition to its others when `adding`, else replacing them) and take
    /// the clashing bindings away from the actions that have them, only those (KEY-018); an action left
    /// without a key is stored as cleared (KEY-007). Nothing changes when a rule refuses `b`.
    pub fn overwrite(&mut self, a: Action, b: Binding, adding: bool) -> Result<(), String> {
        self.check_rules(a, &b)?;
        for other in self.clashes_of(a, &b) {
            let kept = self.bindings(other).into_iter().filter(|o| !o.clashes(&b)).collect();
            self.store(other, kept);
        }
        if adding {
            self.add(a, b)
        } else {
            self.set(a, b)
        }
    }

    /// Give `a` this one binding (replacing its others); nothing changes when it is refused.
    pub fn set(&mut self, a: Action, b: Binding) -> Result<(), String> {
        self.check(a, &b)?;
        self.store(a, vec![b]);
        Ok(())
    }

    /// Give `a` this binding in addition to its others (KEY-006); nothing changes when it is refused.
    pub fn add(&mut self, a: Action, b: Binding) -> Result<(), String> {
        self.check(a, &b)?;
        let mut all = self.bindings(a);
        if !all.contains(&b) {
            all.push(b);
        }
        self.store(a, all);
        Ok(())
    }

    /// Leave `a` without a key; it stays so until it is reset (KEY-007).
    pub fn clear(&mut self, a: Action) {
        self.custom.insert(a, vec![]);
    }

    /// Keep these bindings of `a` as a change, or forget the change when they are the defaults.
    fn store(&mut self, a: Action, all: Vec<Binding>) {
        if a.defaults_for(self.mac) == all {
            self.custom.remove(&a);
        } else {
            self.custom.insert(a, all);
        }
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

    /// The keys of the sequence that waits for its next key at time `now`, e.g. `Space f`, for
    /// the status bar; `None` when nothing waits or the wait is over (KEY-009).
    pub fn pending_text(&self, now: f64) -> Option<(String, f64)> {
        self.pending.values().find(|(c, at)| !c.is_empty() && now - at < SEQUENCE_TIMEOUT).map(|(c, at)| (Binding(c.clone()).display(), SEQUENCE_TIMEOUT - (now - at)))
    }

    /// The action of this scope whose binding was just completed by the keys pressed this frame
    /// (the keys are consumed). Starting a sequence returns `None` and remembers its chords; a
    /// key that continues no sequence forgets them (KEY-009).
    pub fn poll(&mut self, input: &mut InputState, scope: Scope) -> Option<Action> {
        if self.recording {
            return None;
        }
        let now = input.time;
        if self.pending.get(&scope).is_some_and(|(_, at)| now - at >= SEQUENCE_TIMEOUT) {
            self.pending.remove(&scope);
        }
        let pending: Vec<Chord> = self.pending.get(&scope).map(|(c, _)| c.clone()).unwrap_or_default();
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
            Some(c) => {
                let entry = self.pending.entry(scope).or_default();
                entry.0.push(c);
                entry.1 = now;
            }
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

    /// The keymap of Linux and Windows, whatever system the tests run on (KEY-015: macOS has no Alt defaults).
    fn linux() -> Keymap {
        Keymap::for_os(false)
    }

    fn b(s: &str) -> Binding {
        Binding::parse(s).unwrap()
    }

    // spec: KEY-001
    #[test]
    fn every_default_parses_and_has_no_conflict() {
        // on macOS the Alt defaults are simply not bound, but their strings still parse and check
        let keymaps = [Keymap::for_os(false), Keymap::for_os(true)];
        let mut ids = std::collections::HashSet::new();
        for d in DEFS {
            assert!(ids.insert(d.id), "double id {}", d.id);
            assert!(!d.defaults.is_empty(), "{} has no default", d.id);
            for text in d.defaults.iter().chain(d.action.mac_defaults().unwrap_or(&[])) {
                let bind = Binding::parse(text).unwrap_or_else(|e| panic!("{}: {e}", d.id));
                assert_eq!(bind.to_text(), *text, "defaults are written the way they are stored");
                for k in &keymaps {
                    assert_eq!(k.check(d.action, &bind), Ok(()), "{} {text}", d.id);
                }
            }
        }
    }

    // spec: KEY-002
    #[test]
    fn the_definable_actions_are_the_listed_ones() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/spec/links-and-editor.md")).unwrap();
        let row = text.lines().find(|l| l.starts_with("| KEY-002 |")).unwrap();
        let (k, mac) = (linux(), Keymap::for_os(true));
        for d in DEFS.iter().filter(|d| d.scope == Scope::Global) {
            let shown = k.display(d.action);
            assert!(row.contains(&shown), "KEY-002 does not name {shown} ({})", d.id);
            // macOS shows the same, except the Alt keys of the ribbon and the sidebars (not set) and the keys of KEY-019
            let on_mac = match d.action.mac_defaults() {
                _ if d.action.is_alt_key() => "not set".to_string(),
                Some(keys) => keys.iter().map(|t| b(t).display()).collect::<Vec<_>>().join(" / "),
                None => shown,
            };
            assert_eq!(mac.display(d.action), on_mac, "{}", d.id);
        }
        assert_eq!(DEFS.iter().filter(|d| d.scope == Scope::NoteView).count(), 24 + 5);
        assert_eq!(DEFS.len(), 29 + 36 + 6 + 12 + 24 + 5);
    }

    // spec: KEY-014
    #[test]
    fn the_note_view_overlaps_global_and_vim_normal_mode_both_ways_and_alt_arrows_are_the_editors() {
        let mut k = linux();
        // the editor's own Alt+Up / Alt+Down (moving rows) cannot start a binding there, also not in a sequence
        for text in ["Alt+Up", "Alt+Down", "Alt+Up X"] {
            assert!(k.check(Action::NoteBold, &b(text)).unwrap_err().contains("fixed meaning"), "{text}");
        }
        assert_eq!(k.check(Action::NoteBold, &b("Alt+B")), Ok(()), "its own key");
        // a binding that would type is refused, as in Global
        assert!(k.check(Action::NoteBold, &b("G")).unwrap_err().contains("needs Ctrl or Alt"));
        // against Global ...
        assert_eq!(k.check(Action::NoteBold, &b("Alt+Left")), Err("Alt+← is already used by “Back”".to_string()));
        assert_eq!(k.check(Action::Save, &b("Alt+B")), Err("Alt+B is already used by “Note: Bold button”".to_string()));
        // ... and against vim-style normal mode, from both sides
        k.set(Action::NormalFindFile, b("Ctrl+Alt+Q")).unwrap();
        assert_eq!(k.check(Action::NoteBold, &b("Ctrl+Alt+Q")), Err("Ctrl+Alt+Q is already used by “Find a file by name”".to_string()));
        assert_eq!(k.check(Action::NormalSearchAll, &b("Alt+I")), Err("Alt+I is already used by “Note: Italic button”".to_string()));
        assert_eq!(k.check(Action::NormalSearchAll, &b("Alt+B Alt+B")), Err("Alt+B Alt+B is already used by “Note: Bold button”".to_string()));
        // the other scopes do not see the note view's keys: the mail view has Alt+I of its own
        assert_eq!(k.check(Action::TreeNextFile, &b("Alt+B")), Ok(()));
        assert_eq!(k.check(Action::MailBtnImport, &b("Alt+I")), Ok(()));
        // the overlap is symmetric for every pair of scopes
        for a in Scope::ALL {
            for c in Scope::ALL {
                assert_eq!(a.overlaps(c), c.overlaps(a), "{a:?} {c:?}");
            }
        }
        // on macOS, where Alt+B is no default, the Alt keys are free, and Alt+Up stays the editor's
        let mut m = Keymap::for_os(true);
        assert_eq!(m.check(Action::Save, &b("Alt+B")), Ok(()));
        assert_eq!(m.check(Action::NormalSearchAll, &b("Alt+I")), Ok(()));
        assert!(m.check(Action::NoteBold, &b("Alt+Up")).unwrap_err().contains("fixed meaning"));
        m.set(Action::NoteBold, b("Alt+B")).unwrap();
        assert_eq!(m.check(Action::Save, &b("Alt+B")), Err("Alt+B is already used by “Note: Bold button”".to_string()));
        // a saved change that clashes is ignored on load (KEY-007)
        let saved: BTreeMap<String, Vec<String>> = [("note.bold".to_string(), vec!["Alt+Left".to_string()]), ("normal.find_file".to_string(), vec!["Alt+I".to_string()])].into_iter().collect();
        assert!(Keymap::from_overrides_for(&saved, false).overrides().is_empty());
    }

    // spec: KEY-015
    #[test]
    fn the_alt_keys_of_the_buttons_have_defaults_on_linux_and_windows_but_none_on_macos() {
        let (linux, mac) = (Keymap::for_os(false), Keymap::for_os(true));
        let alt: Vec<&Def> = DEFS.iter().filter(|d| d.action.is_alt_key()).collect();
        assert_eq!(alt.len(), 24 + 12 + 9);
        for d in &alt {
            assert!(!linux.bindings(d.action).is_empty(), "{} has a default on Linux", d.id);
            assert!(mac.bindings(d.action).is_empty(), "{} has none on macOS", d.id);
            assert!(matches!(d.scope, Scope::NoteView | Scope::Global | Scope::MailView));
        }
        // everything else has the same keys on both, except the six actions of KEY-019
        for d in DEFS.iter().filter(|d| !d.action.is_alt_key() && d.action.mac_defaults().is_none()) {
            assert_eq!(linux.bindings(d.action), mac.bindings(d.action), "{}", d.id);
        }
        assert_eq!(linux.bindings(Action::NoteBold), vec![b("Alt+B")]);
        assert_eq!(linux.bindings(Action::RibbonSlot10), vec![b("Alt+0")]);
        assert_eq!(linux.display(Action::RibbonSlot1), "Alt+1");
        assert_eq!(linux.display(Action::SidebarLeft), "Alt+,");
        assert_eq!(linux.display(Action::SidebarRight), "Alt+.");
        assert_eq!(Action::RibbonSlot3.ribbon_slot(), Some(2));
        assert_eq!(Action::NoteBold.ribbon_slot(), None);
        // KEY-007: a change is what differs from the defaults of the system
        let mut m = Keymap::for_os(true);
        m.set(Action::NoteBold, b("Alt+B")).unwrap();
        assert_eq!(m.overrides().get("note.bold"), Some(&vec!["Alt+B".to_string()]), "on macOS Alt+B is a change");
        let mut l = Keymap::for_os(false);
        l.set(Action::NoteBold, b("Alt+B")).unwrap();
        assert!(l.overrides().is_empty(), "on Linux it is the default");
        l.clear(Action::NoteBold);
        assert_eq!(l.overrides().get("note.bold"), Some(&vec![]));
        // what a Mac user saved is read back as it was
        let back = Keymap::from_overrides_for(&m.overrides(), true);
        assert_eq!(back.bindings(Action::NoteBold), vec![b("Alt+B")]);
        // the badge and underline text of a single Alt chord
        assert_eq!(linux.alt_badge(Action::NoteBold).as_deref(), Some("B"));
        assert_eq!(linux.alt_badge(Action::RibbonSlot1).as_deref(), Some("1"));
        assert_eq!(linux.alt_badge(Action::SidebarLeft).as_deref(), Some(","));
        assert_eq!(linux.alt_badge(Action::Save), None, "Ctrl+S is no Alt key");
        assert_eq!(mac.alt_badge(Action::NoteBold), None);
    }

    // spec: KEY-030, KEY-031, KEY-032, KEY-033, KEY-034
    #[test]
    fn the_fold_keys_have_the_same_defaults_on_every_system_and_clash_with_nothing() {
        let (linux, mac) = (Keymap::for_os(false), Keymap::for_os(true));
        for (a, key) in [(Action::NoteFold, "Ctrl+Shift+K"), (Action::NoteUnfold, "Ctrl+Shift+J"), (Action::NoteFoldAll, "Ctrl+Shift+O"), (Action::NoteUnfoldAll, "Ctrl+Shift+I")] {
            assert!(!a.is_alt_key(), "{key} is no Alt key");
            for k in [&linux, &mac] {
                assert_eq!(k.bindings(a), vec![b(key)]);
                assert_eq!(k.check(a, &b(key)), Ok(()));
            }
        }
        assert_eq!(linux.check(Action::NoteFold, &b("Ctrl+Shift+F")), Err("Ctrl+Shift+F is already used by “Search all notes”".to_string()));
    }

    // spec: KEY-035, KEY-036
    #[test]
    fn the_copy_code_key_is_the_same_on_every_system_and_clashes_with_nothing() {
        let (linux, mac) = (Keymap::for_os(false), Keymap::for_os(true));
        assert!(!Action::NoteCopyCode.is_alt_key());
        for k in [&linux, &mac] {
            assert_eq!(k.bindings(Action::NoteCopyCode), vec![b("Ctrl+Shift+C")]);
            assert_eq!(k.check(Action::NoteCopyCode, &b("Ctrl+Shift+C")), Ok(()));
        }
    }

    // spec: KEY-017
    #[test]
    fn no_two_actions_of_scopes_that_are_active_together_clash_on_any_system() {
        let sets: [&[Scope]; 5] = [&[Scope::Global, Scope::FileList], &[Scope::Global, Scope::SearchResults], &[Scope::Global, Scope::MailView], &[Scope::Global, Scope::NoteView], &[Scope::Global, Scope::NoteView, Scope::EditorNormal]];
        for (system, mac) in [("linux", false), ("windows", false), ("macos", true)] {
            let k = Keymap::for_os(mac);
            for set in sets {
                let actions: Vec<&Def> = DEFS.iter().filter(|d| set.contains(&d.scope)).collect();
                for (i, x) in actions.iter().enumerate() {
                    for y in &actions[i + 1..] {
                        for bx in k.bindings(x.action) {
                            for by in k.bindings(y.action) {
                                assert!(!bx.clashes(&by), "{system}: {} and {} both use {}", x.id, y.id, bx.display());
                            }
                        }
                    }
                }
            }
        }
        // the sets are every combination that can be active together: no other two scopes overlap
        for a in Scope::ALL {
            for c in Scope::ALL {
                let together = a == c || sets.iter().any(|s| s.contains(&a) && s.contains(&c));
                assert_eq!(a.overlaps(c), together, "{a:?} {c:?}");
            }
        }
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
        let mut k = linux();
        // the message names the other action
        assert_eq!(k.check(Action::FocusEditor, &b("Ctrl+H")), Err("Ctrl+H is already used by “Keyboard one place to the left (pane, then left sidebar)”".to_string()));
        // a Global key shadows the other scopes ...
        assert_eq!(k.check(Action::TreeNextFile, &b("Ctrl+H")), Err("Ctrl+H is already used by “Keyboard one place to the left (pane, then left sidebar)”".to_string()));
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

    // spec: KEY-018
    #[test]
    fn clashes_of_finds_equal_and_prefix_clashes_in_scopes_that_are_active_together() {
        let mut k = linux();
        // equal, in the same scope and against Global
        assert_eq!(k.clashes_of(Action::TreePrevFile, &b("J")), vec![Action::TreeNextFile]);
        assert_eq!(k.clashes_of(Action::TreeNextFile, &b("J")), vec![], "its own key is no clash");
        assert_eq!(k.clashes_of(Action::TreeNextFile, &b("Ctrl+H")), vec![Action::FocusSidebar]);
        // Note view against Global, and the other way round
        assert_eq!(k.clashes_of(Action::NoteBold, &b("Ctrl+E")), vec![Action::ToggleReading]);
        assert_eq!(k.clashes_of(Action::Save, &b("Alt+B")), vec![Action::NoteBold]);
        // Note view against vim normal mode
        k.set(Action::NoteBold, b("Ctrl+Alt+K")).unwrap();
        assert_eq!(k.clashes_of(Action::NormalAlternate, &b("Ctrl+Alt+K")), vec![Action::NoteBold]);
        assert_eq!(k.clashes_of(Action::NoteBold, &b("Space Tab")), vec![Action::NormalAlternate]);
        // unrelated scopes do not clash
        assert_eq!(k.clashes_of(Action::HitNext, &b("J")), vec![]);
        // a prefix clash names every action: Space f against Space f f and Space f g
        k.set(Action::TreeToggleFolder, b("Space f f")).unwrap();
        k.set(Action::TreePrevFile, b("Space f g")).unwrap();
        assert_eq!(k.clashes_of(Action::TreeNextFile, &b("Space f")), vec![Action::TreePrevFile, Action::TreeToggleFolder]);
        // the rules of KEY-005 are no clashes
        assert_eq!(k.clashes_of(Action::Save, &b("G")), vec![]);
        assert!(k.check_rules(Action::Save, &b("G")).is_err());
    }

    // spec: KEY-018, KEY-007
    #[test]
    fn overwrite_takes_only_the_clashing_bindings_and_stores_an_emptied_action_as_cleared() {
        let mut k = linux();
        let down = k.bindings(Action::TreeNextFile);
        assert!(down.len() > 1, "the action has more than one key by default");
        k.overwrite(Action::TreePrevFile, b("J"), false).unwrap();
        assert_eq!(k.bindings(Action::TreePrevFile), vec![b("J")]);
        assert_eq!(k.bindings(Action::TreeNextFile), down.into_iter().filter(|d| !d.clashes(&b("J"))).collect::<Vec<_>>(), "its other keys stay");
        // an action left without a key is stored as cleared and survives the config
        k.overwrite(Action::TreeToggleFolder, b("Ctrl+Alt+R"), false).unwrap();
        let only = k.bindings(Action::NewNote);
        assert_eq!(only.len(), 1, "New note has one key by default");
        k.overwrite(Action::FocusSidebar, only[0].clone(), true).unwrap();
        assert_eq!(k.bindings(Action::NewNote), vec![]);
        assert_eq!(k.overrides().get("global.new_note"), Some(&vec![]));
        assert!(k.bindings(Action::FocusSidebar).contains(&only[0]), "adding keeps the other keys");
        let k2 = Keymap::from_overrides_for(&k.overrides(), false);
        assert_eq!(k2.bindings(Action::NewNote), vec![]);
        assert_eq!(k2.display(Action::NewNote), "not set");
        assert_eq!(k2.overrides(), k.overrides());
        // a prefix clash takes both sequences
        k.set(Action::TreeToggleFolder, b("Space f f")).unwrap();
        k.set(Action::HitNext, b("Space f g")).unwrap();
        k.overwrite(Action::TreeNextFile, b("Space f"), false).unwrap();
        assert_eq!(k.bindings(Action::TreeToggleFolder), vec![]);
        assert_eq!(k.bindings(Action::TreeNextFile), vec![b("Space f")]);
        assert_eq!(k.bindings(Action::HitNext), vec![b("Space f g")], "another scope, kept");
        // a rule still refuses and changes nothing
        let before = k.overrides();
        assert!(k.overwrite(Action::Save, b("Ctrl+C"), false).unwrap_err().contains("reserved"));
        assert_eq!(k.overrides(), before);
    }

    // spec: KEY-005
    #[test]
    fn reserved_and_plain_global_keys_are_refused() {
        let k = linux();
        assert!(k.check(Action::Save, &b("G")).unwrap_err().contains("needs Ctrl or Alt"));
        assert!(k.check(Action::Save, &b("F5")).is_ok(), "a function key does not get in the way of typing");
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
        let k = linux();
        let a = Action::NormalAlternate;
        for text in ["H", "Shift+W", "G", "0", "4", "Slash", "Colon", "Escape", "Enter", "Backspace", "ArrowDown", "G X", "Shift+O Tab", "U", "Shift+U", "Ctrl+R", "Ctrl+R X"] {
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
        let mut k = linux();
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
        let mut k = linux();
        assert!(k.overrides().is_empty(), "only changes are stored");
        k.set(Action::FocusSidebar, b("Ctrl+Alt+B")).unwrap();
        k.set(Action::TreeToggleFolder, b("Space f f")).unwrap();
        k.set(Action::Save, b("Ctrl+S")).unwrap();
        assert_eq!(k.overrides().len(), 2, "a binding put back to its default is no change");
        let mut cfg = crate::config::Config { keybindings: k.overrides(), ..Default::default() };
        let dir = std::env::temp_dir().join(format!("shale-keys-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");
        crate::config::write_private(&path, crate::config::text::to_toml(&cfg).as_bytes()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"global.focus_sidebar\" = [\"Ctrl+Alt+B\"]") && text.contains("version = 2"), "{text}");
        let (loaded, problem) = crate::config::Config::load_from(&path);
        assert_eq!(problem, None);
        let k2 = Keymap::from_overrides_for(&loaded.keybindings, false);
        assert_eq!(k2.bindings(Action::FocusSidebar), vec![b("Ctrl+Alt+B")]);
        assert_eq!(k2.bindings(Action::TreeToggleFolder), vec![b("Space f f")]);
        assert_eq!(k2.bindings(Action::FocusEditor), Action::FocusEditor.defaults_for(false));
        // a file without the field means all defaults
        cfg.keybindings.clear();
        assert!(Keymap::from_overrides_for(&cfg.keybindings, false).overrides().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: KEY-007
    #[test]
    fn bad_overrides_in_the_config_are_ignored() {
        let saved: BTreeMap<String, Vec<String>> = [
            ("global.nonsense", vec!["Ctrl+Q"]),
            ("global.back", vec!["Ctrl+Hyper"]),
            ("global.forward", vec![]),
            // takes the key of "Keyboard one place to the left (pane, then left sidebar)", which keeps it
            ("global.close_tab", vec!["Ctrl+H"]),
            ("global.new_note", vec!["Ctrl+Alt+N"]),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.into_iter().map(String::from).collect()))
        .collect();
        let k = Keymap::from_overrides_for(&saved, false);
        // an empty list is a cleared action (KEY-007), not a bad entry
        assert_eq!(k.overrides().keys().collect::<Vec<_>>(), ["global.forward", "global.new_note"]);
        assert!(k.is_default(Action::Back) && k.is_default(Action::CloseTab));
    }

    // spec: MAIL-072
    #[test]
    fn a_hint_is_the_bindings_as_shown_and_nothing_for_an_unbound_action() {
        let mut k = linux();
        assert_eq!(k.hint(Action::MailFlag).as_deref(), Some("s / f"));
        assert_eq!(k.hint(Action::MailRetry).as_deref(), Some("Shift+S"));
        k.set(Action::MailRetry, Binding::parse("G").unwrap()).unwrap();
        assert_eq!(k.hint(Action::MailRetry).as_deref(), Some("g"));
        k.clear(Action::MailFlag);
        assert_eq!(k.hint(Action::MailFlag), None);
    }

    // spec: KEY-007, KEY-006
    #[test]
    fn a_cleared_action_stays_unbound() {
        let mut k = linux();
        k.clear(Action::MailFlag);
        assert!(k.bindings(Action::MailFlag).is_empty() && !k.is_default(Action::MailFlag));
        assert_eq!(k.display(Action::MailFlag), "not set");
        assert_eq!(frame(&mut k, Scope::MailView, vec![key_event(Key::S, Modifiers::NONE)]), None, "s does nothing now");
        assert_eq!(frame(&mut k, Scope::MailView, vec![key_event(Key::F, Modifiers::NONE)]), None, "nor does f");
        // stored as an empty list, read back as unbound
        assert_eq!(k.overrides().get("mail.flag"), Some(&vec![]));
        let k2 = Keymap::from_overrides_for(&k.overrides(), false);
        assert!(k2.bindings(Action::MailFlag).is_empty());
        // the freed keys can go to another action; reset brings the defaults back
        let mut k2 = k2;
        k2.set(Action::MailRead, b("S")).unwrap();
        k2.reset(Action::MailFlag);
        assert_eq!(k2.check(Action::MailFlag, &b("S")), Err("s is already used by “Mark read or unread”".to_string()));
        k.reset(Action::MailFlag);
        assert!(k.overrides().is_empty());
    }

    // spec: KEY-006
    #[test]
    fn add_keeps_the_other_bindings() {
        let mut k = linux();
        k.add(Action::TreeNextFile, b("Ctrl+Alt+J")).unwrap();
        assert_eq!(k.bindings(Action::TreeNextFile), vec![b("J"), b("Down"), b("Ctrl+Alt+J")]);
        k.add(Action::TreeNextFile, b("Ctrl+Alt+J")).unwrap();
        assert_eq!(k.bindings(Action::TreeNextFile).len(), 3, "the same binding twice is one");
        // a refused one changes nothing and names the other action
        assert_eq!(k.add(Action::TreeNextFile, b("K")), Err("k is already used by “Previous file”".to_string()));
        assert_eq!(k.bindings(Action::TreeNextFile).len(), 3);
        // adding the missing default of a changed action ends in "not changed"
        k.set(Action::MailFlag, b("S")).unwrap();
        k.add(Action::MailFlag, b("F")).unwrap();
        assert!(k.is_default(Action::MailFlag));
    }

    // spec: KEY-010
    #[test]
    fn a_bare_key_does_not_fire_with_shift_or_alt() {
        let mut k = linux();
        let sc = Scope::MailView;
        // the Alt key of a letter is the action of the button of the open mail (MAIL-229), never the bare key's
        for (key, bare, shift, alt) in [
            (Key::S, Some(Action::MailFlag), Some(Action::MailRetry), None),
            (Key::R, Some(Action::MailReply), Some(Action::MailReplyAll), Some(Action::MailBtnReply)),
            (Key::F, Some(Action::MailFlag), Some(Action::MailForward), Some(Action::MailBtnForward)),
            (Key::D, Some(Action::MailDelete), None, Some(Action::MailBtnDelete)),
        ] {
            assert_eq!(frame(&mut k, sc, vec![key_event(key, Modifiers::NONE)]), bare, "{key:?}");
            assert_eq!(frame(&mut k, sc, vec![key_event(key, Modifiers::SHIFT)]), shift, "Shift+{key:?}");
            assert_eq!(frame(&mut k, sc, vec![key_event(key, Modifiers::ALT)]), alt, "Alt+{key:?}");
        }
        // in a sequence too: a later chord needs its exact modifiers
        k.set(Action::MailMove, b("G X")).unwrap();
        assert_eq!(frame(&mut k, sc, vec![key_event(Key::G, Modifiers::NONE)]), None);
        assert_eq!(frame(&mut k, sc, vec![key_event(Key::X, Modifiers::SHIFT)]), None);
        assert_eq!(frame(&mut k, sc, vec![key_event(Key::G, Modifiers::NONE)]), None);
        assert_eq!(frame(&mut k, sc, vec![key_event(Key::X, Modifiers::NONE)]), Some(Action::MailMove));
    }

    // spec: KEY-009
    #[test]
    fn a_waiting_sequence_is_shown_and_times_out() {
        let ctx = egui::Context::default();
        let mut k = linux();
        k.set(Action::TreeToggleFolder, b("Space f f")).unwrap();
        let at = |k: &mut Keymap, t: f64, key: Key| {
            let mut out = None;
            let mut full = ctx.run_ui(RawInput { time: Some(t), events: vec![key_event(key, Modifiers::NONE)], ..Default::default() }, |ui| out = ui.input_mut(|i| k.poll(i, Scope::FileList)));
            full.textures_delta.clear();
            out
        };
        assert_eq!(k.pending_text(10.0), None);
        at(&mut k, 10.0, Key::Space);
        at(&mut k, 10.2, Key::F);
        assert_eq!(k.pending_text(10.5).map(|p| p.0), Some("Space f".to_string()));
        let left = k.pending_text(10.5).unwrap().1;
        assert!((left - 1.2).abs() < 1e-9, "{left}");
        assert_eq!(k.pending_text(11.8), None, "after 1.5 s the status bar drops it");
        // the dropped sequence does not complete late: the last f starts nothing
        assert_eq!(at(&mut k, 12.0, Key::F), None);
        assert_eq!(k.pending_text(12.1), None);
        // within the time it completes, and a completed one shows nothing
        at(&mut k, 20.0, Key::Space);
        at(&mut k, 21.0, Key::F);
        assert_eq!(at(&mut k, 22.4, Key::F), Some(Action::TreeToggleFolder));
        assert_eq!(k.pending_text(22.5), None);
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

    // spec: KEY-005, EDIT-033
    #[test]
    fn shift_k_is_the_link_preview_and_space_space_can_replace_it() {
        let mut k = linux();
        let a = Action::NormalLinkPreview;
        assert_eq!(k.bindings(a), vec![b("Shift+K")], "the default");
        assert_eq!(k.check(a, &b("Shift+K")), Ok(()), "Shift+K is no vim key");
        // plain k (up) and the other shifted vim keys stay fixed
        for text in ["K", "Shift+J", "Shift+O"] {
            assert!(k.check(a, &b(text)).unwrap_err().contains("fixed meaning"), "{text}");
        }
        // Space Space is free: no clash with Space Tab, Space f f, Space f g
        assert_eq!(k.check(a, &b("Space Space")), Ok(()));
        assert_eq!(k.check(Action::NormalAlternate, &b("Space Tab")), Ok(()));
        // the default fires on Shift+K, not on k
        let none = Modifiers::NONE;
        assert_eq!(frame(&mut k, Scope::EditorNormal, vec![key_event(Key::K, Modifiers::SHIFT)]), Some(a));
        assert_eq!(frame(&mut k, Scope::EditorNormal, vec![key_event(Key::K, none)]), None, "plain k stays vim's");
        // rebound: only the new sequence fires, the others still do
        k.set(a, b("Space Space")).unwrap();
        assert!(k.check(Action::NormalFindFile, &b("Space Space")).unwrap_err().contains("already used"), "the others cannot take it from the action");
        assert_eq!(frame(&mut k, Scope::EditorNormal, vec![key_event(Key::K, Modifiers::SHIFT)]), None);
        assert_eq!(frame(&mut k, Scope::EditorNormal, vec![key_event(Key::Space, none)]), None);
        assert_eq!(frame(&mut k, Scope::EditorNormal, vec![key_event(Key::Space, none)]), Some(a));
        assert_eq!(frame(&mut k, Scope::EditorNormal, vec![key_event(Key::Space, none)]), None);
        assert_eq!(frame(&mut k, Scope::EditorNormal, vec![key_event(Key::Tab, none)]), Some(Action::NormalAlternate));
        assert_eq!(k.overrides().get("normal.link_preview"), Some(&vec!["Space Space".to_string()]), "stored as a change (KEY-007)");
    }

    // spec: KEY-009, KEY-008
    #[test]
    fn a_sequence_fires_on_its_last_chord() {
        let mut k = linux();
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
        let k = linux();
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
            // the keys macOS has instead (KEY-019), written the Mac way: Cmd and Opt for Ctrl and Alt
            for d in DEFS.iter().filter(|d| d.action.mac_defaults().is_some()) {
                for bind in Keymap::for_os(true).bindings(d.action) {
                    let shown = bind.display().replace("Ctrl+", "Cmd+").replace("Alt+", "Opt+");
                    assert!(table.contains(&shown), "{file}: the Keyboard table does not list {shown} for macOS ({})", d.label);
                }
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum Os {
        Linux,
        Windows,
        Mac,
    }

    /// Why the chord is no default key on this system (KEY-019): what the system, the desktop or the
    /// app menu keeps for itself, or `None` when it is free. `Chord.ctrl` is Cmd on a Mac. For Linux
    /// it is the union of GNOME, KDE, XFCE, Cinnamon and MATE; Super is never a modifier of Shale.
    fn os_reserved(os: Os, scope: Scope, c: Chord) -> Option<&'static str> {
        let (ctrl, alt, shift, key) = (c.ctrl, c.alt, c.shift, c.key);
        let arrow = matches!(key, Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown);
        let function = (Key::F1..=Key::F12).contains(&key);
        match os {
            Os::Mac => match (ctrl, alt, shift, key) {
                (true, false, false, Key::H) => Some("Hide (app menu)"),
                (true, true, false, Key::H) => Some("Hide Others (app menu)"),
                (true, false, false, Key::Q) => Some("Quit (app menu)"),
                (true, false, true, Key::Q) => Some("Log out"),
                (true, false, false, Key::M) => Some("Minimise"),
                (true, false, false, Key::Tab) => Some("Switch app"),
                (true, false, false, Key::Space) => Some("Spotlight"),
                (true, true, false, Key::Space) => Some("Finder search"),
                (true, false, true, Key::Num3 | Key::Num4 | Key::Num5) => Some("Screenshot"),
                (true, false, false, Key::Backtick) => Some("Next window of the app"),
                (true, true, false, Key::Escape) => Some("Force Quit"),
                (true, true, false, Key::D) => Some("Dock"),
                (true, false, true, Key::Slash) => Some("Help menu"),
                // word and paragraph moves in text, which a shortcut runs before
                (false, true, false, _) if arrow && matches!(scope, Scope::Global | Scope::NoteView) => Some("Option+arrow moves in text"),
                _ => None,
            },
            Os::Windows => match (ctrl, alt, shift, key) {
                (false, true, false, Key::F4) => Some("close window"),
                (false, true, false, Key::Tab) => Some("switch window"),
                (false, true, false, Key::Space) => Some("window menu"),
                (false, true, false, Key::Escape) => Some("next window"),
                (true, false, false, Key::Escape) => Some("Start menu"),
                (true, false, true, Key::Escape) => Some("Task Manager"),
                (true, true, false, Key::Delete) => Some("security screen"),
                // AltGr arrives as Ctrl+Alt and types a character on many layouts
                (true, true, _, k) if k.symbol_or_name().chars().count() == 1 && !arrow => Some("AltGr + character"),
                _ => None,
            },
            Os::Linux => match (ctrl, alt, shift, key) {
                (true, true, false, _) if arrow => Some("switch workspace"),
                (true, true, false, Key::L) => Some("lock screen"),
                (true, true, false, Key::T) => Some("terminal"),
                (true, true, false, Key::D) => Some("show desktop"),
                (true, true, false, Key::Delete) => Some("log out"),
                (true, false, true, Key::U) => Some("Unicode input of IBus and GTK"),
                (true, true, false, Key::Backspace) => Some("kill the X server"),
                (true, true, false, Key::Escape) => Some("kill a window"),
                (false, true, false, _) if function => Some("Alt+F1…F12 of the desktops"),
                (false, true, false, Key::Tab) => Some("switch window"),
                (false, true, false, Key::Space) => Some("window menu"),
                (false, true, false, Key::Escape) => Some("next window"),
                (false, true, false, Key::Backtick) => Some("next window of the app"),
                _ => None,
            },
        }
    }

    // spec: KEY-019
    #[test]
    fn no_default_key_is_one_the_system_reserves_on_linux_windows_or_macos() {
        for (os, mac) in [(Os::Linux, false), (Os::Windows, false), (Os::Mac, true)] {
            let k = Keymap::for_os(mac);
            let mut checked = 0;
            for d in DEFS {
                for bind in k.bindings(d.action) {
                    checked += 1;
                    let first = bind.0[0];
                    assert_eq!(os_reserved(os, d.scope, first), None, "{os:?}: the default {} of {} is reserved", bind.display(), d.id);
                }
            }
            assert!(checked > 60, "{os:?}: {checked} defaults checked");
        }
    }

    // spec: KEY-019
    #[test]
    fn the_lists_of_reserved_keys_catch_the_old_defaults_and_not_the_new_ones() {
        let c = |t: &str| Chord::parse(t).unwrap();
        // what these defaults were: each is reserved on the system it was reported for
        for (os, scope, old) in [
            (Os::Mac, Scope::Global, "Ctrl+H"),
            (Os::Mac, Scope::Global, "Ctrl+Alt+H"),
            (Os::Mac, Scope::Global, "Alt+Left"),
            (Os::Windows, Scope::Global, "Ctrl+Alt+W"),
            (Os::Windows, Scope::Global, "Ctrl+Alt+L"),
            (Os::Linux, Scope::Global, "Ctrl+Alt+Left"),
            (Os::Linux, Scope::Global, "Ctrl+Alt+Right"),
            (Os::Linux, Scope::Global, "Ctrl+Alt+L"),
            (Os::Linux, Scope::NoteView, "Ctrl+Shift+U"),
        ] {
            assert!(os_reserved(os, scope, c(old)).is_some(), "{os:?} {old}");
        }
        // Alt+Left is the Back key on Linux and Windows, and Option+arrow only matters on a Mac
        assert_eq!(os_reserved(Os::Linux, Scope::Global, c("Alt+Left")), None);
        assert_eq!(os_reserved(Os::Windows, Scope::Global, c("Alt+Left")), None);
        assert_eq!(os_reserved(Os::Mac, Scope::FileList, c("Alt+Left")), None);
        // Ctrl+Alt+arrows are no AltGr keys on Windows; Ctrl+Shift+W is free everywhere
        assert_eq!(os_reserved(Os::Windows, Scope::Global, c("Ctrl+Alt+Left")), None);
        for os in [Os::Linux, Os::Windows, Os::Mac] {
            assert_eq!(os_reserved(os, Scope::Global, c("Ctrl+Shift+W")), None);
        }
    }

    // spec: KEY-019
    #[test]
    fn macos_has_its_own_defaults_for_six_actions_and_linux_and_windows_keep_theirs() {
        let (linux, mac) = (Keymap::for_os(false), Keymap::for_os(true));
        for (action, on_linux, on_mac) in [
            (Action::FocusSidebar, "Ctrl+H", "Ctrl+Shift+H"),
            (Action::FocusEditor, "Ctrl+L", "Ctrl+Shift+L"),
            (Action::Back, "Alt+Left", "Ctrl+OpenBracket"),
            (Action::Forward, "Alt+Right", "Ctrl+CloseBracket"),
            (Action::PrevTab, "Ctrl+PageUp", "Ctrl+Alt+Left"),
            (Action::NextTab, "Ctrl+PageDown", "Ctrl+Alt+Right"),
        ] {
            assert_eq!(linux.bindings(action), vec![b(on_linux)], "{action:?} on Linux");
            assert_eq!(mac.bindings(action), vec![b(on_mac)], "{action:?} on macOS");
            assert!(linux.is_default(action) && mac.is_default(action));
        }
        // the pane keys are the same on both, and close pane only differs in what Ctrl means
        assert_eq!(linux.bindings(Action::PrevPane), vec![b("Shift+F6")]);
        assert_eq!(linux.bindings(Action::NextPane), vec![b("F6")]);
        assert_eq!(mac.bindings(Action::PrevPane), vec![b("Shift+F6")]);
        assert_eq!(mac.bindings(Action::NextPane), vec![b("F6")]);
        assert_eq!(linux.bindings(Action::ClosePane), vec![b("Ctrl+Shift+W")]);
        assert_eq!(mac.bindings(Action::ClosePane), vec![b("Ctrl+Shift+W")]);
        assert_eq!(mac.display(Action::Back), "Ctrl+[", "Chord.ctrl is Cmd on a Mac, the docs say Cmd+[");
        // the new keys pass the rules of KEY-005 (Shift+F6 is a function key)
        for action in [Action::PrevPane, Action::NextPane, Action::PrevTab, Action::NextTab, Action::ClosePane] {
            for k in [&linux, &mac] {
                for bind in k.bindings(action) {
                    assert_eq!(k.check_rules(action, &bind), Ok(()), "{action:?} {}", bind.display());
                }
            }
        }
        // the user's own binding is not refused for being reserved (KEY-019): Ctrl+Alt+Left works on KDE
        let mut l = Keymap::for_os(false);
        assert_eq!(l.set(Action::PrevPane, b("Ctrl+Alt+Left")), Ok(()));
        assert_eq!(l.overrides().get("global.prev_pane"), Some(&vec!["Ctrl+Alt+Left".to_string()]));
    }
}
