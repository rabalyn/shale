//! The registry of the settings (CFG-071): one table with a row for every setting the Settings
//! dialog draws, `(id, tab, label, …)`. The dialog's search reads it (CFG-072), and the drawing
//! code names a row with `anchor::anchor(ui, id, …)`, which is what lets a search result scroll to
//! the setting and pulse a box around it (CFG-073). Three tests keep the table whole: every row
//! has an anchor in the code, every anchor has a row on the tab it is drawn on, and every key of
//! the settings file is claimed by a row or listed as one that has no place in the dialog
//! (`tests` below).
//!
//! The actions of the Keybindings tab are not rows of the table: they are the rows of
//! `keys::DEFS`, searched by their label and the keys they are bound to now.

use super::SettingsTab::{self, *};
use crate::keys::{Action, Def, Keymap, DEFS};
use std::sync::OnceLock;

/// One setting of the dialog.
pub struct Entry {
    /// Names the anchor in the drawing code: `<tab>.<name>`.
    pub id: &'static str,
    pub tab: SettingsTab,
    /// What the setting is called in the results of the search.
    pub label: &'static str,
    /// The keys of the settings file this row edits (nested keys joined with `.`); the search also
    /// finds a setting by their description (CFG-009).
    pub keys: &'static [&'static str],
    /// More words that find it: what it is also called, what it controls.
    pub words: &'static str,
    /// The row to pulse when this one is not drawn (a font of an area while one font is used for
    /// all, the light theme while no light theme is chosen).
    pub fallback: Option<&'static str>,
}

const fn e(id: &'static str, tab: SettingsTab, label: &'static str, keys: &'static [&'static str], words: &'static str) -> Entry {
    Entry { id, tab, label, keys, words, fallback: None }
}

impl Entry {
    const fn or(mut self, fallback: &'static str) -> Entry {
        self.fallback = Some(fallback);
        self
    }

    /// The description of the setting in the settings file (CFG-009), the first key's.
    pub fn help(&self) -> &'static str {
        self.keys.first().and_then(|k| crate::config::text::description(&k.split('.').map(String::from).collect::<Vec<_>>())).unwrap_or("")
    }
}

/// Every setting of the dialog, in the order of the tabs.
pub const ENTRIES: &[Entry] = &[
    e("vault.folder", Vault, "Vault folder", &["vault"], "open switch directory path"),
    e("vault.close", Vault, "Close this vault", &[], "start screen"),
    e("vault.people_dir", Vault, "People folder", &[], "contacts people notes link [[p"),
    e("vault.names", Vault, "Check names for Windows and macOS", &[], "file names rename compatible other systems"),
    e("editor.cursor_shape", Editor, "Cursor while typing", &["cursor_shape"], "caret bar block underline"),
    e("editor.cursor_blink", Editor, "Blinking cursor", &["cursor_blink"], "caret blink"),
    e("editor.line_numbers", Editor, "Show line numbers in the editor", &["line_numbers"], "gutter"),
    e("editor.numbered_headings", Editor, "Number headings", &["numbered_headings"], "chapters 1.1"),
    e("editor.date_prefix", Editor, "Date prefix for new notes", &["date_prefix_new_notes"], "YYMMDD file name new note"),
    e("editor.live_preview", Editor, "Markdown preview in the editor", &["live_preview"], "live preview rendered raw"),
    e("editor.preview_raw_lines", Editor, "Raw lines around the cursor", &["preview_raw_lines"], "markdown preview"),
    e("editor.table_cell_edit", Editor, "Edit table cells in the grid", &["table_cell_edit"], "tables"),
    e("editor.ctrl_to_follow", Editor, "Follow links only with Ctrl+click", &["ctrl_to_follow"], "click open link"),
    e("editor.new_notes_next_to_current", Editor, "Create notes from red links next to the current note", &["new_notes_next_to_current"], "broken link vault root folder"),
    e("editor.update_links_on_rename", Editor, "Update links in other notes when renaming", &["update_links_on_rename"], "rename move"),
    e("editor.auto_convert_canvas", Editor, "Convert Obsidian canvases to editable drawings", &["auto_convert_canvas"], "canvas excalidraw drawing"),
    e("editor.format_toolbar", Editor, "Show formatting toolbar", &["format_toolbar"], "buttons bold italic"),
    e("editor.toolbar_size", Editor, "Toolbar button size", &["toolbar_size"], "formatting toolbar scale"),
    e("editor.font_colours", Editor, "Font colours", &["font_colours"], "palette toolbar text colour color"),
    e("editor.highlight_colours", Editor, "Highlight colours", &["highlight_colours"], "palette toolbar marker background colour color"),
    e("vim.mode", Vim, "Vim-style editing", &["vim_mode"], "vi normal insert visual modal"),
    e("vim.normal_cursor", Vim, "Cursor in normal mode", &["normal_cursor_shape"], "caret bar block underline vim"),
    e("vim.esc_cursor", Vim, "Esc from insert mode", &["esc_cursor"], "caret step back vim"),
    e("vim.link_preview_size", Vim, "Link preview size", &["link_preview_width", "link_preview_height"], "Shift+K width height reset"),
    e("appearance.theme", Appearance, "Theme", &["theme.name"], "colours colors scheme dark light alacritty").or("appearance.follow_system"),
    e("appearance.follow_system", Appearance, "Follow the system's light or dark mode", &["theme.follow_system"], "theme automatic"),
    e("appearance.theme_light", Appearance, "Light theme", &["theme.light"], "colours colors scheme system").or("appearance.follow_system"),
    e("appearance.theme_dark", Appearance, "Dark theme", &["theme.dark"], "colours colors scheme system").or("appearance.follow_system"),
    e("appearance.themes_folder", Appearance, "Themes folder", &[], "open reload own theme files toml"),
    e("appearance.readable_width", Appearance, "Readable line length", &["readable_width"], "column width text"),
    e("appearance.dark_canvas", Appearance, "Dark drawing canvas", &["dark_canvas"], "drawings background"),
    e("appearance.properties_editor", Appearance, "Properties in the editor", &["properties_in_editor"], "front matter folded card raw"),
    e("appearance.properties_reader", Appearance, "Properties in the reading view", &["properties_in_reader"], "front matter folded card raw"),
    e("appearance.inline_images", Appearance, "Show embedded images and players inside the editor", &["inline_images"], "pictures media"),
    e("appearance.play_button", Appearance, "Play button over the artwork of a sound or video", &["media_play_button_opacity"], "opacity media player"),
    e("fonts.same", Fonts, "Use the same font and size everywhere", &["fonts.same_everywhere"], "one font all areas"),
    e("fonts.all", Fonts, "Font and size (all areas)", &["fonts.all"], "family text size points").or("fonts.same"),
    e("fonts.ui", Fonts, "Font: dialogs and menus", &["fonts.ui"], "family text size points sidebars").or("fonts.same"),
    e("fonts.editor_raw", Fonts, "Font: editor, raw markdown", &["fonts.editor_raw"], "family text size points monospace").or("fonts.same"),
    e("fonts.editor_rendered", Fonts, "Font: editor, rendered text and reading view", &["fonts.editor_rendered"], "family text size points preview").or("fonts.same"),
    e("fonts.mail", Fonts, "Font: mail", &["fonts.mail"], "family text size points").or("fonts.same"),
    e("fonts.calendar", Fonts, "Font: calendar", &["fonts.calendar"], "family text size points").or("fonts.same"),
    e("fonts.social", Fonts, "Font: feeds", &["fonts.social"], "family text size points social").or("fonts.same"),
    e("sync.git_choice", Sync, "Git to use", &["git.choice"], "git program system managed lfs"),
    e("sync.git_install", Sync, "Shale's own git", &["git.updates"], "install update remove managed git-lfs").or("sync.git_choice"),
    e("sync.turn_on", Sync, "Turn on sync", &[], "wizard repository remote server forgejo gitea github gitlab"),
    e("sync.remote", Sync, "Remote address of the repository", &[], "advanced connect url server").or("sync.turn_on"),
    e("sync.automatic", Sync, "Sync this vault automatically", &[], "git every minutes").or("sync.turn_on"),
    e("sync.commit_message", Sync, "Commit message prefix and suffix", &[], "git").or("sync.turn_on"),
    e("sync.lfs", Sync, "Store media with Git LFS", &[], "audio video large files").or("sync.turn_on"),
    e("sync.lfs_download", Sync, "Download media files after every sync", &[], "git lfs on demand").or("sync.turn_on"),
    e("sync.now", Sync, "Sync now", &[], "git").or("sync.turn_on"),
    e("history.enabled", Sync, "Keep earlier versions of my notes (file history)", &["history.enabled"], "snapshots go back undo"),
    e("history.timing", Sync, "How often a version of a note is kept", &["history.coalesce_minutes", "history.idle_seconds"], "file history minutes seconds idle"),
    e("history.deleted", Sync, "Restore a deleted note", &[], "file history trash undelete"),
    e("calendar.accounts", Calendar, "Calendars", &["calendars"], "caldav ics local accounts add"),
    e("calendar.week_start", Calendar, "Calendar weeks start on Monday", &["week_starts_monday"], "sunday first day"),
    e("mail.accounts", Mail, "Mail accounts", &["mail_accounts"], "imap smtp add sending encryption"),
    e("mail.sync_minutes", Mail, "Fetch new mail every (minutes)", &["mail_sync_minutes"], "interval"),
    e("mail.mark_read", Mail, "Opening a message marks it read on the server", &["mail_mark_read"], "seen"),
    e("mail.mark_read_secs", Mail, "Mark a message read after (seconds)", &["mail_mark_read_secs"], "delay"),
    e("social.accounts", Social, "Feed accounts", &["social_accounts"], "mastodon bluesky social add"),
    e("social.check_minutes", Social, "Look for new posts every (minutes)", &["social_check_minutes"], "interval feeds"),
    e("social.index", Social, "Post search index", &[], "rebuild clear cache posts"),
    e("search.sources", Search, "Sources a search looks in", &["search.sources"], "notes mail posts default selection"),
    e("search.hit_display", Search, "Hits of notes", &["search.hit_display"], "results rendered raw follow lines"),
    e("search.prefixes", Search, "Prefixes of a query", &["search.prefix_notes", "search.prefix_mail", "search.prefix_posts"], "letters f: m: s: colon"),
    e("import.outside_files", Import, "Files from outside the vault", &["outside_files"], "copy link ask picture pdf sound video paste pick"),
    e("import.libraries", Import, "Library folders", &["media_libraries"], "media videos @name folder computer"),
    e("import.header_list", Import, "Header list in the note body", &["import_header_list"], "e-mails events posts properties"),
    e("import.skip_signatures", Import, "Skip signatures", &["import_skip_signatures"], "e-mails signature smime pgp logo picture attachments"),
    e("import.leave_out_quoted", Import, "Leave out quoted mails", &["import_leave_out_quoted"], "e-mails thread reply quote history wrote original message"),
    e("import.docx", Import, "Import Word files as notes", &["docx_import"], "docx pandoc attachment"),
    e("import.pandoc", Import, "pandoc program", &["pandoc_path"], "docx word check again"),
    e("data.usage", Data, "Data usage", &[], "disk space caches what Shale keeps"),
    e("data.retention", Data, "How long caches are kept", &["caches"], "retention pictures posts index backups undo debug log days megabytes"),
    e("tex.setup", Latex, "Set up LaTeX export", &[], "guided setup templates logo"),
    e("tex.engine", Latex, "TeX to use", &["tex.engine"], "lualatex biber auto system managed"),
    e("tex.test", Latex, "Test the TeX", &[], "compile check again"),
    e("tex.managed", Latex, "Shale's own TeX Live", &[], "install update remove download"),
    e("tex.sets", Latex, "Template sets", &[], "latex templates import vault"),
    e("tex.repository", Latex, "Download address of TeX Live", &["tex.repository"], "repository mirror"),
    e("updates.mode", Updates, "When an update is found", &["updates.mode"], "release ask off notify download install automatic"),
    e("updates.check", Updates, "Check for updates now", &[], "new release version"),
    e("help.guide", Help, "Open user guide", &[], "F1 manual documentation"),
    e("help.input_log", Help, "Record keyboard input to a file", &["input_log"], "troubleshooting keyboard problems log"),
];

/// Keys of the settings file that no row claims, with why. A key is also covered when a row or
/// this list names a key it starts with (`sidebars` covers `sidebars.ribbon`).
#[cfg(test)]
pub const NOT_IN_THE_DIALOG: &[(&str, &str)] = &[
    ("version", "the format of the file"),
    ("vaults", "the known vaults: the start screen and the vault menu"),
    ("show_left", "the left panel is toggled in the window"),
    ("show_right", "the right sidebar is toggled in the window"),
    ("sidebars", "arranged in the window: the context menu of the ribbon and the sidebar"),
    ("file_sort", "chosen in the file tree"),
    ("settings_tab_width", "dragged: the border of the tab list"),
    ("left_sidebar_width", "dragged: the border of the sidebar"),
    ("right_sidebar_width", "dragged: the border of the sidebar"),
    ("media_volume", "the volume slider of the players"),
    ("social_last", "the feed shown last"),
    ("social_replies", "the last choice in the Import dialog"),
    ("mail_undo_secs", "the delay of the Mail settings dialog"),
    ("mail_encryption_consent", "given in the Mail settings dialog"),
    ("history.show_panel", "the X of the history column and the history button"),
    ("updates.skipped_version", "set by Skip this version in the update question"),
    ("updates.last_check", "kept by the update check"),
    ("updates.install_on_start", "kept by the update install"),
    ("keybindings", "the Keybindings tab: one row per action, searched by its label and its keys"),
];

/// A row of the results: a setting of the table or an action of the Keybindings tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// The anchor: the id of a row of the table, or `key:<action id>`.
    pub id: String,
    pub tab: SettingsTab,
    pub label: &'static str,
    /// For an action: the keys it is bound to now, as the tab shows them.
    pub keys: Option<String>,
    pub action: Option<Action>,
    score: u32,
}

/// The anchor of an action's row on the Keybindings tab.
pub fn action_anchor(def: &Def) -> String {
    format!("key:{}", def.id)
}

/// The keys of an action as the Keybindings tab shows them (`Ctrl+E`, `j / ↓`), what a search by
/// key looks in; empty for an action without a key (the tab says "not set", which is no key).
pub fn bound_keys(keymap: &Keymap, action: Action) -> String {
    if keymap.bindings(action).is_empty() {
        String::new()
    } else {
        keymap.display(action)
    }
}

/// What is typed, as the terms it searches for: lower case, split at white space.
pub fn terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

/// The text of the search, lower case, made once.
struct Indexed {
    label: String,
    words: String,
    help: String,
    tab: String,
}

fn index() -> &'static [Indexed] {
    static INDEX: OnceLock<Vec<Indexed>> = OnceLock::new();
    INDEX.get_or_init(|| ENTRIES.iter().map(|e| Indexed { label: e.label.to_lowercase(), words: e.words.to_lowercase(), help: e.help().to_lowercase(), tab: e.tab.label().to_lowercase() }).collect())
}

/// How well one term fits a setting: the label counts most (a word that starts it more), then the
/// extra words, the description and the name of the tab; `None`: not at all.
fn term_score(i: &Indexed, term: &str) -> Option<u32> {
    if i.label.starts_with(term) {
        Some(5)
    } else if i.label.contains(term) {
        Some(4)
    } else if i.words.contains(term) {
        Some(3)
    } else if i.help.contains(term) {
        Some(2)
    } else if i.tab.contains(term) {
        Some(1)
    } else {
        None
    }
}

/// Whether an action stays in the list of the Keybindings tab for the terms typed (CFG-072,
/// KEY-006): each term is a part of its label or of the keys it is bound to as the tab shows them
/// (`ctrl+h`, `f6`), case ignored. No terms: every action.
pub fn action_matches(label: &str, keys: &str, terms: &[String]) -> bool {
    action_score(label, keys, terms).is_some()
}

fn action_score(label: &str, keys: &str, terms: &[String]) -> Option<u32> {
    let (label, keys) = (label.to_lowercase(), keys.to_lowercase());
    terms.iter().try_fold(0, |sum, t| {
        let s = if label.starts_with(t.as_str()) {
            5
        } else if label.contains(t.as_str()) || keys.contains(t.as_str()) {
            4
        } else {
            return None;
        };
        Some(sum + s)
    })
}

/// The settings and actions that fit what is typed, the best first, at most `LIMIT`; settings
/// before actions of the same score. Nothing for an empty query.
pub fn search(query: &str, keymap: &Keymap) -> Vec<Hit> {
    let terms = terms(query);
    if terms.is_empty() {
        return vec![];
    }
    let mut hits: Vec<Hit> = ENTRIES
        .iter()
        .zip(index())
        .filter_map(|(e, i)| {
            let score = terms.iter().try_fold(0, |sum, t| term_score(i, t).map(|s| sum + s))?;
            Some(Hit { id: e.id.to_string(), tab: e.tab, label: e.label, keys: None, action: None, score })
        })
        .collect();
    hits.extend(DEFS.iter().filter_map(|d| {
        let score = action_score(d.label, &bound_keys(keymap, d.action), &terms)?;
        Some(Hit { id: action_anchor(d), tab: Keybindings, label: d.label, keys: Some(keymap.display(d.action)), action: Some(d.action), score })
    }));
    // a stable sort keeps the table's order among equals
    hits.sort_by_key(|h| std::cmp::Reverse(h.score));
    hits.truncate(LIMIT);
    hits
}

/// How many results the list shows at most.
pub const LIMIT: usize = 40;

/// The row of the table with this id.
pub fn entry(id: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    fn keymap() -> Keymap {
        Keymap::from_overrides_for(&Default::default(), false)
    }

    fn ids(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.id.as_str()).collect()
    }

    /// Every `.rs` file below `src`, without the tests of the table itself.
    fn sources() -> Vec<(PathBuf, String)> {
        fn walk(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push((p.clone(), std::fs::read_to_string(&p).unwrap()));
                }
            }
        }
        let mut out = vec![];
        walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
        out
    }

    /// The ids named by `anchor(ui, "…"`, `mark(ui, "…"` and `anchor_id("…"` calls in the code.
    fn anchored() -> HashSet<String> {
        let mut ids = HashSet::new();
        for (path, text) in sources() {
            if path.ends_with("settings/registry.rs") || path.ends_with("settings/anchor.rs") {
                continue;
            }
            for call in ["anchor(ui, \"", "mark(ui, \"", "anchor_id(\""] {
                let mut rest = text.as_str();
                while let Some(at) = rest.find(call) {
                    rest = &rest[at + call.len()..];
                    ids.insert(rest[..rest.find('"').unwrap()].to_string());
                }
            }
        }
        ids
    }

    // spec: CFG-071
    #[test]
    fn cfg_071_every_row_of_the_table_has_an_anchor_in_the_code_and_every_anchor_a_row() {
        let drawn = anchored();
        let mut seen = HashSet::new();
        for e in ENTRIES {
            assert!(seen.insert(e.id), "the id {} is used twice", e.id);
            assert_eq!(e.id.split('.').count(), 2, "{} is <group>.<name>", e.id);
            assert!(!e.label.is_empty());
            assert!(drawn.contains(e.id), "the row {} has no anchor in the code: draw the setting inside anchor(ui, \"{}\", …)", e.id, e.id);
            if let Some(f) = e.fallback {
                assert!(entry(f).is_some_and(|f| f.tab == e.tab && f.fallback.is_none()), "{}: the fallback {f} is a row of the same tab that has none itself", e.id);
            }
        }
        for id in &drawn {
            assert!(entry(id).is_some(), "the anchor {id} has no row in settings/registry.rs");
        }
    }

    /// Every key of the settings file with its description: a row of the table claims it, or the
    /// list of what the dialog does not show names it.
    fn covered(key: &str) -> bool {
        let under = |k: &str| key == k || key.starts_with(&format!("{k}."));
        // (the table that holds claimed keys is claimed by them)
        let parent = |k: &str| k.starts_with(&format!("{key}."));
        let claimed = || ENTRIES.iter().flat_map(|e| e.keys).copied().chain(NOT_IN_THE_DIALOG.iter().map(|(k, _)| *k));
        claimed().any(|k| under(k) || parent(k))
    }

    // spec: CFG-071
    #[test]
    fn cfg_071_every_setting_of_the_file_is_in_the_table_or_listed_as_not_in_the_dialog() {
        let keys = crate::config::text::described_keys();
        assert!(keys.len() > 100, "the descriptions are read");
        let missing: Vec<&str> = keys.iter().copied().filter(|k| !covered(k)).collect();
        assert!(missing.is_empty(), "these settings of the file are neither a row of settings/registry.rs (ENTRIES, keys) nor in NOT_IN_THE_DIALOG: {missing:?}");
        // and nothing named there is gone from the file
        let known = |k: &str| keys.iter().any(|w| *w == k || w.starts_with(&format!("{k}.")));
        for k in ENTRIES.iter().flat_map(|e| e.keys).chain(NOT_IN_THE_DIALOG.iter().map(|(k, _)| k)) {
            assert!(known(k), "{k} is named in settings/registry.rs but the settings file has no such key");
        }
        // the last two settings added by other changes are found
        assert!(covered("outside_files") && covered("media_libraries.path"));
    }

    // spec: CFG-072
    #[test]
    fn cfg_072_a_setting_is_found_by_its_label_its_words_and_its_description() {
        let k = keymap();
        let hits = search("blinking", &k);
        assert_eq!(ids(&hits)[0], "editor.cursor_blink", "by label");
        assert_eq!(hits[0].tab, Editor);
        // the case does not matter, the words may be anywhere in the label
        assert_eq!(ids(&search("CURSOR blink", &k))[0], "editor.cursor_blink");
        // by a word of its own that the label does not have
        assert!(ids(&search("alacritty", &k)).contains(&"appearance.theme"));
        // by the description of the settings file (CFG-009): "comfortable column" is only there
        assert_eq!(ids(&search("comfortable column", &k)), ["appearance.readable_width"]);
        // the settings added by the media changes
        let outside = search("outside the vault", &k);
        assert!(ids(&outside).contains(&"import.outside_files"), "{:?}", ids(&outside));
        assert!(ids(&search("library", &k)).contains(&"import.libraries"));
        // the name of a tab lists its settings, after those that name the word themselves
        let mail = search("mail", &k);
        assert!(ids(&mail).contains(&"mail.sync_minutes"));
        assert!(search("zzzzz", &k).is_empty());
        assert!(search("   ", &k).is_empty(), "nothing typed, nothing listed");
    }

    // spec: CFG-072
    #[test]
    fn cfg_072_an_action_is_found_by_its_label_and_by_the_keys_it_is_bound_to_as_the_tab_shows_them() {
        let k = keymap();
        let by_keys = search("ctrl+h", &k);
        let a = DEFS.iter().find(|d| k.display(d.action).to_lowercase().split(" / ").any(|b| b == "ctrl+h")).expect("an action is bound to Ctrl+H");
        let hit = by_keys.iter().find(|h| h.action == Some(a.action)).unwrap_or_else(|| panic!("{:?}", ids(&by_keys)));
        assert_eq!((hit.tab, hit.id.as_str(), hit.keys.as_deref()), (Keybindings, format!("key:{}", a.id).as_str(), Some(k.display(a.action).as_str())));
        let f6 = DEFS.iter().find(|d| k.display(d.action) == "F6").expect("an action is bound to F6");
        assert!(search("F6", &k).iter().any(|h| h.action == Some(f6.action)), "case ignored");
        assert!(search("f6", &k).iter().any(|h| h.action == Some(f6.action)));
        // by label, and the label of the settings does not hide it
        assert!(search("reading view", &k).iter().any(|h| h.action == Some(Action::ToggleReading)));
        // a cleared action is not found by "not set"
        let mut k2 = keymap();
        k2.clear(Action::ToggleReading);
        assert!(!search("not set", &k2).iter().any(|h| h.action == Some(Action::ToggleReading)));
        assert!(action_matches("Toggle reading view", "Ctrl+E", &terms("reading ctrl+e")), "every term must fit");
        assert!(!action_matches("Toggle reading view", "Ctrl+E", &terms("reading ctrl+x")));
        assert!(action_matches("Anything", "x", &[]), "no terms: every action");
    }

    // spec: CFG-072
    #[test]
    fn cfg_072_the_best_fit_comes_first_and_the_list_is_short() {
        let k = keymap();
        let hits = search("theme", &k);
        assert_eq!(hits[0].id, "appearance.theme", "the label that starts with the word first");
        assert!(search("e", &k).len() <= LIMIT);
    }
}
