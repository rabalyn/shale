//! The settings file as text (CFG-009): TOML with a description comment above every setting.
//! The file is written by `to_toml` and read by `decode`; `Config` itself is still serde, so the
//! file goes through the same versioned loading as the vault's JSON files (CFG-004, CFG-007).

use super::Config;
use serde_json::{Map, Value};

/// The first lines of the file.
const HEADER: &str = "\
# Shale settings (this computer, all vaults).
#
# Edit this file in Shale (Settings, \"Open settings file\") or in any other editor; Shale
# notices the change. Shale writes the whole file again whenever a setting changes and
# writes the comments again too, so comments you add yourself do not stay. Settings that
# this version of Shale does not know are kept as they are. A line you remove takes its
# default. Passwords and tokens are kept in the system keyring when there is one.
";

/// The comment above a setting this version of Shale does not know.
const UNKNOWN: &str = "Not known to this version of Shale (a newer version, or typed by hand); kept as it is.";

/// What the cache settings say for "forever" / "no limit": TOML has no null.
const NONE_WORD: &str = "none";

/// Every setting of the file with its description: the key (nested keys joined with `.`; the
/// entries of a list of tables count as one) and what it does. A setting the file can hold has
/// a row here, or the test `cfg_009_every_key_of_the_file_has_a_description` fails.
const DESCRIPTIONS: &[(&str, &str)] = &[
    ("version", "Format version of this file. Do not change it."),
    ("vault", "The vault that was open last; it opens again at the next start."),
    ("recent", "The vaults opened lately, newest first (at most 8)."),
    ("ctrl_to_follow", "true: a link is followed only with Ctrl+click; false: a plain click follows it."),
    ("new_notes_next_to_current", "true: a note made from a red link goes next to the current note; false: into the vault's top folder."),
    ("update_links_on_rename", "true: renaming a note changes the links to it in other notes."),
    ("dark_canvas", "true: drawings are drawn on a dark canvas."),
    ("theme", "The colours of the window. A theme is a built-in one or an Alacritty theme file in the themes folder next to this file (Settings > Appearance)."),
    ("theme.name", "The theme by name, such as \"Shale\", \"Dracula\" or the name of a file in the themes folder. An unknown name gives Shale."),
    ("theme.follow_system", "true: the theme follows the system's light or dark mode, using light and dark below instead of name."),
    ("theme.light", "The theme used while the system is in light mode (needs follow_system)."),
    ("theme.dark", "The theme used while the system is in dark mode (needs follow_system)."),
    ("fonts", "The fonts and text sizes. With same_everywhere on, [fonts.all] is used everywhere and the tables of the single areas are not; with it off, each area has its own."),
    ("fonts.same_everywhere", "true: one font and size (fonts.all) for every area; false: each area uses its own table below."),
    ("fonts.all", "Used for every area while same_everywhere is true. The size is the size of dialogs and menus in points; the editor's text is drawn 11 % larger, as it always was."),
    ("fonts.all.family", "The name of an installed font family such as \"DejaVu Sans\"; leave the line out for Shale's own font. A font that is not installed is replaced by Shale's own."),
    ("fonts.all.size", "The text size in points, 8 to 48."),
    ("fonts.ui", "Dialogs, menus, sidebars and everything not listed below."),
    ("fonts.ui.family", "The name of an installed font family such as \"DejaVu Sans\"; leave the line out for Shale's own font. A font that is not installed is replaced by Shale's own."),
    ("fonts.ui.size", "The text size in points, 8 to 48."),
    ("fonts.editor_raw", "The raw markdown text in the editor, and the raw lines around the cursor in the preview."),
    ("fonts.editor_raw.family", "The name of an installed font family such as \"DejaVu Sans\"; leave the line out for Shale's own font. A font that is not installed is replaced by Shale's own."),
    ("fonts.editor_raw.size", "The text size in points, 8 to 48."),
    ("fonts.editor_rendered", "Rendered text: the lines of the markdown preview and the reading view."),
    ("fonts.editor_rendered.family", "The name of an installed font family such as \"DejaVu Sans\"; leave the line out for Shale's own font. A font that is not installed is replaced by Shale's own."),
    ("fonts.editor_rendered.size", "The text size in points, 8 to 48."),
    ("fonts.mail", "The mail view."),
    ("fonts.mail.family", "The name of an installed font family such as \"DejaVu Sans\"; leave the line out for Shale's own font. A font that is not installed is replaced by Shale's own."),
    ("fonts.mail.size", "The text size in points, 8 to 48."),
    ("fonts.calendar", "The calendar views."),
    ("fonts.calendar.family", "The name of an installed font family such as \"DejaVu Sans\"; leave the line out for Shale's own font. A font that is not installed is replaced by Shale's own."),
    ("fonts.calendar.size", "The text size in points, 8 to 48."),
    ("fonts.social", "The feed view."),
    ("fonts.social.family", "The name of an installed font family such as \"DejaVu Sans\"; leave the line out for Shale's own font. A font that is not installed is replaced by Shale's own."),
    ("fonts.social.size", "The text size in points, 8 to 48."),
    ("readable_width", "true: the text of a note stays in a comfortable column; false: it uses the full width."),
    ("show_left", "true: the left panel (files, search) is shown."),
    ("show_right", "true: the right sidebar is shown."),
    ("sidebars", "Where the icons of the window are: the ribbon on the left and the views of the right sidebar."),
    ("sidebars.ribbon", "The icons of the ribbon, top to bottom, by id. Empty: the default order."),
    ("sidebars.right", "The views of the right sidebar, top to bottom. None: one view with all panels."),
    ("sidebars.right.icons", "The icons of this view's menu bar, left to right, by id."),
    ("sidebars.right.active", "The id of the icon this view shows."),
    ("sidebars.right.weight", "The share of the sidebar's height this view takes; 0: an equal share."),
    ("sidebars.hidden", "The icons that were removed; the context menu of the ribbon brings them back."),
    ("file_sort", "The order of the file tree: NameAsc, NameDesc, ModifiedDesc, ModifiedAsc, CreatedDesc or CreatedAsc."),
    ("inline_images", "true: pictures show inside the editor; the link shows while the cursor is on it."),
    ("line_numbers", "true: a column with line numbers sits left of the text in the editor."),
    ("date_prefix_new_notes", "true: a note made by Shale (new note, red link, quick switcher) is named YYMMDD_name.md with today's date; its title line shows the name without the prefix."),
    ("properties_in_editor", "How the editor shows the properties (front matter) of a note: Folded (one line), Card or Raw."),
    ("properties_in_reader", "How the reading view shows the properties (front matter) of a note: Folded (one line), Card or Raw."),
    ("numbered_headings", "true: the headings of a note (# and deeper) are shown with numbers 1, 1.1, 1.2; a note's front matter numbered-headings: true or false overrides it."),
    ("format_toolbar", "true: the formatting toolbar is shown above the text of a note."),
    ("toolbar_size", "The size of the toolbar's buttons in percent of the base size, 80 to 200."),
    ("link_preview_width", "The width of the link preview (Shift+K in vim-style editing) in points; dragging its edges changes it. At least 240, at most the window."),
    ("link_preview_height", "The height of the link preview in points; dragging its edges changes it. At least 160, at most the window."),
    ("settings_tab_width", "The width of the tab list of the Settings window in points; dragging its border changes it. At least 60, at most half of the window. Left out: as wide as the longest tab name."),
    ("left_sidebar_width", "The width of the left sidebar in points; dragging its border changes it (at least 170, at most 520). Left out: 260."),
    ("right_sidebar_width", "The width of the right sidebar in points; dragging its border changes it (at least 200, at most 560). Left out: 290."),
    ("font_colours", "The font colours of the toolbar's palette, each as [red, green, blue] (0 to 255)."),
    ("highlight_colours", "The highlight colours of the toolbar's palette, each as [red, green, blue] (0 to 255)."),
    ("live_preview", "true: the editor draws every line but the cursor's rendered (Markdown preview)."),
    ("preview_raw_lines", "How many lines before and after the cursor line stay raw in the preview."),
    ("table_cell_edit", "true: a click on the grid of a table edits that cell in place."),
    ("media_volume", "The volume of the media players, 0.0 to 1.0."),
    ("media_play_button_opacity", "How opaque the round play button over a player's artwork is, 0.0 to 1.0."),
    ("media_libraries", "The library folders of this computer: media outside the vault that notes link as ![[@Name/folder/file.mp4]]. The same name on another computer can be another folder."),
    ("media_libraries.name", "The name notes use in the link, after the @ (letters and digits; capitals do not matter)."),
    ("media_libraries.path", "The folder on this computer that the name stands for."),
    ("outside_files", "What a picked, pasted or imported picture, PDF, sound or video from outside the vault gets: Ask, Copy (a copy in the media folder) or Link (the link points to the file where it is). Other files are always copied."),
    ("calendars", "The calendars of the calendar panel: files on this computer and CalDAV calendars."),
    ("calendars.id", "A name that identifies the calendar; do not reuse it."),
    ("calendars.name", "What the calendar is called in the window."),
    ("calendars.color", "The colour of the calendar's events, as #rrggbb."),
    ("calendars.enabled", "true: the calendar is shown."),
    ("calendars.sync_minutes", "CalDAV: load the calendar again every this many minutes; 0: only at the start and by hand."),
    ("calendars.type", "\"local\" (an .ics file on this computer) or \"caldav\" (a server)."),
    ("calendars.file", "local: the path of the .ics file."),
    ("calendars.url", "caldav: the address of the calendar on the server."),
    ("calendars.username", "caldav: the user name."),
    ("calendars.password", "caldav: the password. It is kept in the system keyring when there is one and then not written here."),
    ("week_starts_monday", "true: calendar weeks start on Monday; false: on Sunday."),
    ("input_log", "Troubleshooting: true records every key and typed text to a file. It is not recorded in password fields."),
    ("vim_mode", "true: notes are edited the way Vim does (normal, insert and visual mode)."),
    ("cursor_shape", "The text cursor while typing: bar (before the character), bar_after, block or underline."),
    ("normal_cursor_shape", "The text cursor in vim's normal mode: bar, bar_after, block or underline."),
    ("cursor_blink", "true: the text cursor blinks; false: it stays on."),
    ("esc_cursor", "Vim-style editing: where Esc from insert mode puts the cursor. stay: on the character right after the typing; step_back: one character back, as in Vim."),
    ("mail_accounts", "The e-mail accounts (IMAP) of the mail view. Passwords are kept in the system keyring, never here."),
    ("mail_accounts.id", "A name that identifies the account (letters, digits, - and _); it is the account's folder in the mail store."),
    ("mail_accounts.name", "What the account is called in the window."),
    ("mail_accounts.host", "The IMAP server."),
    ("mail_accounts.port", "The IMAP port: 993 for ImplicitTls, 143 for StartTls."),
    ("mail_accounts.user", "The user name for the IMAP and SMTP servers."),
    ("mail_accounts.auth", "How the account signs in: Password or OAuth2."),
    ("mail_accounts.security", "How the IMAP connection is secured: ImplicitTls or StartTls. There is no unencrypted choice."),
    ("mail_accounts.oauth_provider", "OAuth2 only: Google, Microsoft or Custom."),
    ("mail_accounts.oauth_client_id", "OAuth2 only: the client ID registered with the provider."),
    ("mail_accounts.oauth_auth_url", "OAuth2 with the Custom provider: the authorization address."),
    ("mail_accounts.oauth_token_url", "OAuth2 with the Custom provider: the token address."),
    ("mail_accounts.oauth_scope", "OAuth2 with the Custom provider: the scope asked for."),
    ("mail_accounts.smtp_host", "The SMTP server that sends mail; empty: sending is not set up."),
    ("mail_accounts.smtp_port", "The SMTP port: 465 for ImplicitTls, 587 for StartTls."),
    ("mail_accounts.smtp_security", "How the SMTP connection is secured: ImplicitTls or StartTls."),
    ("mail_accounts.from_name", "The sender name of sent mail."),
    ("mail_accounts.from_address", "The sender address of sent mail; empty: the user name, when it is an address."),
    ("mail_accounts.signature", "Text added below a new mail and above the quote of a reply."),
    ("mail_accounts.server_stores_sent", "true: the server keeps a copy of sent mail by itself (Gmail), so Shale does not add one."),
    ("mail_accounts.user_agent_header", "true: sent mail carries a User-Agent header (off: more privacy)."),
    ("mail_accounts.pgp_key", "The fingerprint of this account's OpenPGP key in GnuPG; empty: none."),
    ("mail_accounts.smime_key", "The fingerprint of this account's S/MIME certificate in GnuPG; empty: none."),
    ("mail_accounts.sign_by_default", "true: new mail is signed by default."),
    ("mail_accounts.preferred_standard", "The standard a new mail uses to sign and encrypt: OpenPgp or Smime."),
    ("mail_sync_minutes", "The mail view fetches new mail every this many minutes; 0: only with the Sync button."),
    ("mail_mark_read", "true: opening a message in the mail view marks it read on the server."),
    ("mail_mark_read_secs", "... after this many seconds; 0: at once."),
    ("mail_undo_secs", "Seconds a sent mail waits in the outbox, where it can be pulled back; 0: send at once."),
    ("mail_encryption_consent", "When the consent to decrypted local mail storage was given. Without this line encryption is off. Do not add it by hand."),
    ("auto_convert_canvas", "true: opening an Obsidian canvas turns it into an editable drawing without asking."),
    ("docx_import", "true: a Word file (.docx) that is dropped or added in the Import dialog becomes a note, made by pandoc (Settings > Import). false: it is saved as an attachment."),
    ("pandoc_path", "The pandoc program for the Word import. Empty: it is looked for in PATH and the usual places."),
    (
        "import_header_list",
        "true: an imported e-mail, event or social post also gets its properties as a list (From, To, Date, When, Where, Author, Source, ...) below the title. false: the properties are only in the front matter (Settings > Import).",
    ),
    (
        "import_skip_signatures",
        "true: the Import dialog starts with the signature parts of an e-mail (smime.p7s, signature.asc) and the pictures of its signature block unticked. false: they start ticked like other attachments (Settings > Import).",
    ),
    ("social_accounts", "The Mastodon and Bluesky accounts of the feed view. Their tokens are kept in the system keyring when there is one."),
    ("social_accounts.id", "A name that identifies the account, such as \"mastodon:ada@mastodon.social\"."),
    ("social_accounts.network", "\"mastodon\" or \"bluesky\"."),
    ("social_accounts.server", "The https address of the server (the Mastodon instance, or the Bluesky server)."),
    ("social_accounts.handle", "What you sign in with: \"ada@mastodon.social\" or \"ada.bsky.social\"."),
    ("social_accounts.display_name", "What the account is called in the window."),
    ("social_accounts.scopes", "Mastodon: the permissions the token was given at sign-in, such as \"read write\". Without \"write\" Shale asks you to sign in again before it likes, boosts or posts. Not a secret."),
    ("social_accounts.post_language", "The language the last post of this account had, as a two-letter code such as \"de\" (\"\" = none). The next post of the account starts with it; without it the setting social_post_language decides. Not a secret."),
    ("social_accounts.secret", "The access token or app password. It is written here only when there is no system keyring."),
    ("social_last", "The account and feed shown last in the feed view."),
    ("social_replies", "true: saving a post includes the replies of other people (the last choice in the Import dialog)."),
    ("social_post_language", "The language a new post in the feed view starts with, as a two-letter code such as \"de\" or \"en\" (ISO 639-1); \"\" names none. It starts as the language of the system. You can change it for each post in the compose box."),
    ("social_check_minutes", "Minutes between looks for new posts in the feeds open in tabs; 0: never."),
    ("caches", "How long each cache is kept. A number, or \"none\" for forever (ages) or no limit (sizes and counts)."),
    ("caches.picture_days", "Social pictures: days since a picture was fetched."),
    ("caches.picture_mb", "Social pictures: the size of the folder in MB."),
    ("caches.post_days", "Social post index: days, by the date of the post."),
    ("caches.posts_per_account", "Social post index: posts kept per account."),
    ("caches.mail_backups", "Mail index backups: how many of the newest are kept."),
    ("caches.broken_days", "Damaged mail index copies: days, by the date in their name."),
    ("caches.debug_log_mb", "The key recording of the troubleshooting log: its size in MB."),
    ("caches.undo_days", "Undo histories: days since a step was made."),
    ("tex", "The TeX that exports notes to PDF through LaTeX templates."),
    ("tex.engine", "Which TeX is used: Auto (the managed TeX Live when it is installed, else the lualatex of PATH when it is new enough), System (the lualatex of PATH) or Managed (the TeX Live Shale installs)."),
    ("git", "The git that Shale runs for sync, and the git that Shale can install."),
    ("git.choice", "Which git is used: Auto (the git of PATH when there is one, else the one Shale installs), System (the git of PATH) or Managed (the git that Shale installs; where Shale installs only git-lfs, the git of PATH with that git-lfs)."),
    ("git.updates", "What Shale does about new versions of the git it installed: Off (no request is made), Notify (look 30 seconds after the start and weekly, and say when there is one) or Install (also install it). SHALE_UPDATES=off switches it off."),
    ("history", "The file history: Shale keeps earlier versions of your notes so that you can go back."),
    ("history.enabled", "Keep the file history (true) or not (false). Versions already kept stay."),
    ("history.coalesce_minutes", "At most one version of a note is kept in this many minutes while you type (1 to 1440)."),
    ("history.idle_seconds", "A version is also kept when a note has had no change for this many seconds (5 to 3600)."),
    ("history.show_panel", "Show the history of the note in a column beside it when the window is wide enough (the X of the column hides it)."),
    ("tex.repository", "The TeX Live repository the managed install and update download from."),
    ("search", "The search over notes, mail and posts (Ctrl+Shift+F and Space f g): which sources it looks in and the letters that start a query with one source."),
    ("search.prefix_notes", "The one letter that starts a query for notes only: f gives f:banana. Combined letters work too (fm: is notes and mail). A letter that is another source's, or that would spell a field of the mail and post search (to, in), is not accepted."),
    ("search.prefix_mail", "The one letter that starts a query for mail only: m gives m:banana."),
    ("search.prefix_posts", "The one letter that starts a query for posts only: s gives s:banana."),
    ("search.hit_display", "How the lines of notes that a search found are drawn: follow (like the editor shows notes now: rendered when live_preview is on or the note in view is in the reading view, raw otherwise), raw (the markdown as written) or rendered (bold, links by their text, tags, task boxes). The found text is marked in all of them. An unknown word means follow."),
    ("search.sources", "The sources a query without a prefix looks in and lists, from notes, mail and posts. The buttons above the results change it with Shift+click. An empty or unknown list means all three."),
    ("updates", "What Shale does about new releases on GitHub. SHALE_UPDATES=off in the environment switches it off whatever is set here."),
    ("updates.mode", "Ask (not chosen yet: the first update found asks), Off (no request, no notice), Notify (say a new version is out), Download (download and check it in the background) or Install (download, check and install it; you restart when you like)."),
    ("updates.skipped_version", "The version you chose Skip this version for; it is not announced again (empty: none)."),
    ("updates.last_check", "When the last check for a new release succeeded, in seconds since 1970 (0: never)."),
    ("updates.install_on_start", "A downloaded version that is installed at the next start (empty: none)."),
    ("keybindings", "Keys you changed: the id of an action and its keys. An empty list unbinds the action; an action not listed keeps its default."),
    ("keybindings.*", "The keys of this action, such as [\"Ctrl+Shift+P\"]; an empty list leaves the action without a key."),
];

/// The description of a setting by its path in the file (nested keys joined with `.`).
pub fn description(path: &[String]) -> Option<&'static str> {
    let find = |p: &str| DESCRIPTIONS.iter().find(|(k, _)| *k == p).map(|(_, d)| *d);
    if let Some(d) = find(&path.join(".")) {
        return Some(d);
    }
    // the entries of a table whose keys are made by the user (the action ids of the keybindings)
    let (_, parent) = path.split_last()?;
    find(&format!("{}.*", parent.join(".")))
}

/// The keys that have a description, for the test that the Settings dialog knows every setting
/// (`settings/registry.rs`).
#[cfg(test)]
pub fn described_keys() -> Vec<&'static str> {
    DESCRIPTIONS.iter().map(|(k, _)| *k).filter(|k| !k.ends_with('*')).collect()
}

/// Where the description of a setting stands in the table: the order of the file.
fn rank(path: &[String]) -> usize {
    let at = |p: &str| DESCRIPTIONS.iter().position(|(k, _)| *k == p);
    at(&path.join(".")).or_else(|| path.split_last().and_then(|(_, parent)| at(&format!("{}.*", parent.join("."))))).unwrap_or(usize::MAX)
}

/// A key as TOML writes it: bare when it can be, quoted otherwise.
fn key(k: &str) -> String {
    if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        k.to_string()
    } else {
        toml::Value::String(k.to_string()).to_string()
    }
}

fn dotted(path: &[String]) -> String {
    path.iter().map(|k| key(k)).collect::<Vec<_>>().join(".")
}

/// A value on one line.
fn inline(v: &Value) -> String {
    match v {
        Value::Null => format!("\"{NONE_WORD}\""),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => toml::Value::String(s.clone()).to_string(),
        Value::Array(a) => format!("[{}]", a.iter().filter(|v| !v.is_null()).map(inline).collect::<Vec<_>>().join(", ")),
        Value::Object(m) if m.is_empty() => "{}".into(),
        Value::Object(m) => format!("{{ {} }}", m.iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| format!("{} = {}", key(k), inline(v))).collect::<Vec<_>>().join(", ")),
    }
}

/// A value, as one line when it is short, otherwise a list with one entry per line.
fn value_text(v: &Value) -> String {
    let line = inline(v);
    match v {
        Value::Array(a) if line.chars().count() > 80 && a.len() > 1 => {
            let items: Vec<String> = a.iter().filter(|v| !v.is_null()).map(|v| format!("  {},", inline(v))).collect();
            format!("[\n{}\n]", items.join("\n"))
        }
        _ => line,
    }
}

/// `text` as `# ` lines of at most `width` characters.
fn comment(out: &mut String, text: &str) {
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > 92 {
            out.push_str(&format!("# {line}\n"));
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push_str(&format!("# {line}\n"));
    }
}

struct Writer {
    out: String,
    /// the settings without a description
    missing: Vec<String>,
    /// every setting written (tests)
    #[cfg(test)]
    keys: Vec<String>,
    /// a table header was written and nothing after it
    fresh: bool,
}

impl Writer {
    /// A blank line, unless there is one (or a table header, whose settings follow closely).
    fn gap(&mut self) {
        if !self.fresh && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
        self.fresh = false;
    }

    fn describe(&mut self, path: &[String]) {
        #[cfg(test)]
        self.keys.push(path.join("."));
        match description(path) {
            Some(d) => comment(&mut self.out, d),
            None => {
                self.missing.push(path.join("."));
                comment(&mut self.out, UNKNOWN);
            }
        }
    }

    /// A table: its plain settings first, then its tables and lists of tables.
    fn table(&mut self, map: &Map<String, Value>, path: &mut Vec<String>) {
        let is_list = |v: &Value| v.as_array().is_some_and(|a| !a.is_empty() && a.iter().all(|e| e.is_object()));
        // in the order of the table of descriptions (the order of the fields); what is not in it last, by name
        let mut map: Vec<(&String, &Value)> = map.iter().collect();
        map.sort_by_cached_key(|(k, _)| {
            path.push((*k).clone());
            let r = rank(path);
            path.pop();
            (r, (*k).clone())
        });
        for &(k, v) in map.iter().filter(|(_, v)| !v.is_null() && !v.is_object() && !is_list(v)) {
            path.push(k.clone());
            self.gap();
            self.describe(path);
            self.out.push_str(&format!("{} = {}\n", key(k), value_text(v)));
            path.pop();
        }
        for &(k, v) in &map {
            path.push(k.clone());
            match v {
                Value::Object(m) => {
                    self.fresh = false;
                    self.gap();
                    self.describe(path);
                    self.out.push_str(&format!("[{}]\n", dotted(path)));
                    self.fresh = true;
                    self.table(m, path);
                }
                Value::Array(a) if is_list(v) => {
                    self.fresh = false;
                    self.gap();
                    self.describe(path);
                    for (i, e) in a.iter().enumerate() {
                        if i > 0 {
                            self.fresh = false;
                            self.gap();
                        }
                        self.out.push_str(&format!("[[{}]]\n", dotted(path)));
                        self.fresh = true;
                        if let Value::Object(m) = e {
                            self.table(m, path);
                        }
                    }
                }
                _ => {}
            }
            path.pop();
        }
    }
}

/// `config` as the text of the settings file, the settings that had no description and (tests) every setting written.
fn render(config: &Config) -> (String, Vec<String>, Vec<String>) {
    // through the JSON text, so that a number is what serde wrote (an f32 of 0.8 stays 0.8)
    let json = serde_json::to_string(config).unwrap_or_default();
    let mut map = match serde_json::from_str::<Value>(&json) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    };
    // "forever" and "no limit" are written, not left out: a missing line takes the default
    if let Some(Value::Object(caches)) = map.get_mut("caches") {
        for v in caches.values_mut().filter(|v| v.is_null()) {
            *v = Value::String(NONE_WORD.into());
        }
    }
    let mut w = Writer {
        out: String::new(),
        missing: vec![],
        #[cfg(test)]
        keys: vec![],
        fresh: false,
    };
    w.table(&map, &mut vec![]);
    #[cfg(test)]
    let keys = w.keys;
    #[cfg(not(test))]
    let keys = vec![];
    (format!("{HEADER}\n{}", w.out.trim_start_matches('\n')), w.missing, keys)
}

/// `config` as the text of the settings file (CFG-009).
pub fn to_toml(config: &Config) -> String {
    render(config).0
}

/// The text of a settings file as a map for the versioned loading (`vault_state::parse_with`).
pub fn decode(bytes: &[u8]) -> Result<Map<String, Value>, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| format!("not UTF-8 text: {e}"))?;
    let mut map: Map<String, Value> = toml::from_str(text).map_err(|e| e.to_string())?;
    if let Some(Value::Object(caches)) = map.get_mut("caches") {
        for v in caches.values_mut() {
            if v.as_str().is_some_and(|s| matches!(s.trim().to_lowercase().as_str(), "none" | "forever" | "unlimited" | "no limit")) {
                *v = Value::Null;
            }
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CacheRetention, MailAccount, RightViewCfg, SidebarCfg};
    use crate::media::outside::Library;
    use crate::social::{Account, Network, SocialAccountEntry};

    /// A config in which every collection holds something and every setting that can be left out is set.
    fn full() -> Config {
        let fc = |n: &str| crate::fonts::FontChoice { family: Some(n.into()), size: 16.0 };
        use crate::calendar::store::{CalendarSource, SourceKind};
        let mut c = Config {
            vault: Some("/v".into()),
            recent: vec!["/v".into(), "/w".into()],
            fonts: crate::fonts::FontsCfg {
                same_everywhere: false,
                all: crate::fonts::FontChoice { family: Some("Sans".into()), size: 17.0 },
                ui: fc("Ui"),
                editor_raw: fc("Raw"),
                editor_rendered: fc("Rendered"),
                mail: fc("Mail"),
                calendar: fc("Cal"),
                social: fc("Soc"),
            },
            sidebars: SidebarCfg { ribbon: vec!["files".into(), "mail".into()], right: vec![RightViewCfg { icons: vec!["outline".into()], active: "outline".into(), weight: 2 }], hidden: vec!["graph".into()] },
            calendars: vec![
                CalendarSource { id: "a".into(), name: "Home".into(), color: "#ff0000".into(), enabled: true, sync_minutes: 15, kind: SourceKind::Local { file: "/x/a.ics".into() } },
                CalendarSource { id: "b".into(), name: "Work".into(), color: "#00ff00".into(), enabled: false, sync_minutes: 5, kind: SourceKind::Caldav { url: "https://dav".into(), username: "u".into(), password: "p".into() } },
            ],
            mail_accounts: vec![MailAccount { id: "m1".into(), name: "Mail".into(), host: "imap.example.com".into(), user: "ada".into(), signature: "Ada\nLovelace".into(), ..MailAccount::default() }],
            settings_tab_width: Some(150),
            left_sidebar_width: Some(280),
            right_sidebar_width: Some(310),
            mail_encryption_consent: Some("2026-11-14 12:00".into()),
            social_accounts: vec![SocialAccountEntry {
                account: Account { scopes: Some("read write".into()), post_language: Some("de".into()), ..Account::new(Network::Mastodon, "https://m.test", "ada@m.test", "Ada") },
                secret: "tok".into(),
            }],
            social_last: Some(("a".into(), "home".into())),
            media_volume: 0.8,
            outside_files: crate::media::intake::OutsideFiles::Link,
            media_libraries: vec![Library { name: "Videos".into(), path: "/media/ada/Videos".into() }, Library { name: "Pictures".into(), path: "/home/ada/Pictures".into() }],
            ..Config::default()
        };
        c.keybindings.insert("some.action".into(), vec!["Ctrl+K".into()]);
        c.keybindings.insert("cleared".into(), vec![]);
        c
    }

    fn as_json(c: &Config) -> Value {
        serde_json::to_value(c).unwrap()
    }

    // spec: CFG-009, CFG-070
    #[test]
    fn cfg_009_every_key_of_the_file_has_a_description() {
        let (text, missing, keys) = render(&full());
        assert!(missing.is_empty(), "these settings have no description in config/text.rs (DESCRIPTIONS): {missing:?}");
        // and no description is left over for a setting that is gone
        for (k, _) in DESCRIPTIONS.iter().filter(|(k, _)| !k.ends_with('*')) {
            assert!(keys.iter().any(|w| w == k), "{k:?} has a description but is never written:\n{text}");
        }
        // a setting that has none is found
        let mut c = full();
        c.other.insert("brand_new".into(), Value::Bool(true));
        assert_eq!(render(&c).1, ["brand_new"]);
    }

    // spec: CFG-009
    #[test]
    fn cfg_009_the_file_has_a_description_above_every_setting_and_reads_back_the_same() {
        let c = full();
        let text = to_toml(&c);
        assert!(text.starts_with("# Shale settings"), "{text}");
        assert!(text.contains("writes the whole file again") && text.contains("kept as they are"), "the header says how the file is handled");
        let lines: Vec<&str> = text.lines().collect();
        let mut in_list = false;
        let mut in_text = false;
        for (i, l) in lines.iter().enumerate() {
            if in_text || in_list {
                // the rest of a list or of a text over several lines
                in_text ^= l.matches("\"\"\"").count() % 2 == 1;
                in_list = in_list && *l != "]";
                continue;
            }
            in_text = l.matches("\"\"\"").count() % 2 == 1;
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            in_list = l.ends_with('[');
            let again = l.starts_with("[[") && lines[..i].iter().any(|p| p == l);
            assert!(again || lines[i - 1].starts_with('#'), "no description above {l:?}:\n{text}");
        }
        assert!(text.contains("# The font colours of the toolbar"), "{text}");
        assert!(
            text.contains("[[calendars]]\n# A name that identifies the calendar")
                && text.contains("[[mail_accounts]]")
                && text.contains("[sidebars]")
                && text.contains("[[sidebars.right]]")
                && text.contains("[caches]")
                && text.contains("[keybindings]"),
            "{text}"
        );
        assert!(text.contains("size = 17.0") && text.contains("media_volume = 0.8\n"), "numbers are written as they are: {text}");
        let (back, newer) = Config::parse_text(&text).unwrap();
        assert!(!newer);
        assert_eq!(as_json(&back), as_json(&c), "{text}");
        // written again, it is the same text
        assert_eq!(to_toml(&back), text);
    }

    // spec: CFG-009
    #[test]
    fn cfg_009_a_setting_without_a_value_is_left_out_and_forever_is_written_as_none() {
        let c = Config::default();
        assert!(c.vault.is_none() && c.mail_encryption_consent.is_none() && c.social_last.is_none() && c.caches.broken_days.is_none());
        let text = to_toml(&c);
        assert!(!text.contains("null"), "{text}");
        for k in ["vault", "mail_encryption_consent", "social_last"] {
            assert!(!text.lines().any(|l| l.starts_with(&format!("{k} "))), "{k} is left out: {text}");
        }
        assert!(text.contains("broken_days = \"none\""), "forever is written: {text}");
        let (back, _) = Config::parse_text(&text).unwrap();
        assert_eq!(back.caches, c.caches);
        assert_eq!((back.vault, back.social_last, back.mail_encryption_consent), (None, None, None));
        // typed by hand: any of the words, and a number
        let (hand, _) = Config::parse_text("[caches]\npicture_days = \"forever\"\npicture_mb = 12\nundo_days = \"none\"\n").unwrap();
        assert_eq!((hand.caches.picture_days, hand.caches.picture_mb, hand.caches.undo_days), (None, Some(12), None));
        // a line that is left out takes its default
        assert_eq!(hand.caches.post_days, CacheRetention::default().post_days);
    }

    // spec: CFG-009, CFG-007
    #[test]
    fn cfg_009_what_is_not_known_is_kept_and_says_so() {
        let (c, newer) = Config::parse_text("toolbar_size = 18\nmedia_autoplay = true\n[future]\nx = 1\n").unwrap();
        assert!(!newer);
        assert_eq!(c.toolbar_size, 18);
        let text = to_toml(&c);
        let at = |l: &str| text.lines().position(|x| x == l).unwrap_or_else(|| panic!("{l:?} not in\n{text}"));
        assert!(text.lines().nth(at("media_autoplay = true") - 1).unwrap().starts_with("# Not known to this version"));
        assert!(text.lines().nth(at("[future]") - 1).unwrap().starts_with("# Not known to this version"));
        let (back, _) = Config::parse_text(&text).unwrap();
        assert_eq!(back.other, c.other);
        // a file of a newer Shale is recognised by its version
        assert!(Config::parse_text("version = 9\n").unwrap().1);
        // a file that is not valid TOML, or a value of the wrong kind, is an error
        assert!(Config::parse_text("toolbar_size = [").is_err());
        assert!(Config::parse_text("toolbar_size = \"big\"").is_err());
    }

    // spec: CFG-009, SEC-011
    #[test]
    fn cfg_009_the_file_holds_no_password() {
        let mut c = full();
        // the keyring holds the secrets: the entries have none
        c.social_accounts[0].secret = Default::default();
        for s in c.calendars.iter_mut() {
            if let crate::calendar::store::SourceKind::Caldav { password, .. } = &mut s.kind {
                *password = Default::default();
            }
        }
        let text = to_toml(&c);
        for l in text.lines().filter(|l| !l.starts_with('#')) {
            let l = l.to_lowercase();
            // the caldav entry has a place for a password; it is empty while the keyring holds it
            let k = l.split(" =").next().unwrap_or_default();
            assert!(!matches!(k, "secret" | "token" | "access_token" | "refresh_token") && (!k.contains("password") || l == "password = \"\""), "{l}");
        }
        assert!(c.mail_accounts[0].host == "imap.example.com" && text.contains("host = \"imap.example.com\""));
    }

    // spec: CFG-009
    #[test]
    fn cfg_009_keys_that_need_quotes_are_quoted_and_long_lists_take_a_line_per_entry() {
        let mut c = Config::default();
        c.keybindings.insert("tabs.next one".into(), vec!["Ctrl+Tab".into()]);
        c.recent = (0..6).map(|i| format!("/home/someone/Documents/a-long-folder-name/vault-number-{i}").into()).collect();
        let text = to_toml(&c);
        assert!(text.contains("\"tabs.next one\" = [\"Ctrl+Tab\"]"), "{text}");
        assert!(text.contains("recent = [\n  \"/home/someone/Documents/a-long-folder-name/vault-number-0\",\n"), "{text}");
        assert_eq!(Config::parse_text(&text).unwrap().0.recent, c.recent);
        assert_eq!(Config::parse_text(&text).unwrap().0.keybindings, c.keybindings);
    }
}
