Every setting you can change, its default, and where it lives. Back to [[Start here]]; how the
files work is in [[Vault settings]]. Open the Settings window with **Ctrl+,** or the settings
button of the ribbon; it has a tab per domain (Vault, Editor, Appearance, Fonts, Sync, Calendar, Mail, Feeds, Data, LaTeX, Help) and **Keybindings**, and the button
**Open settings file** at the top right (see below). The tab list is as wide as its longest name at any font size; drag its border to change the width, double-click the border to make it automatic again.

Two files hold settings:

- **Global**: `~/.config/shale/config.toml` (this computer, all vaults). On Windows and macOS
  the program's config folder; the settings file tab shows the path.
- **Vault**: `<vault>/.shale/settings.json` (travels with the vault) and
  `<vault>/.shale/state.json` (session state).

A value of the wrong shape in the file loads as its default; fields Shale does not know are kept.
The "Where" column says which window changes a value. The global file is meant to be edited
too: it is plain text with a description above every setting.

## The settings file

**Settings → Open settings file** opens `config.toml` in a tab of its own. It is not a note: it
is not in the file tree, not in any vault and not in the lists of recent files. Change a value,
and the file is saved a moment after you stop typing (or press **Ctrl+S** or **Save**) and the
setting is used at once. You can edit the file with any other program as well; Shale notices the
change within a second.

- Shale writes the whole file again whenever a setting changes, with the descriptions, so
  comments you add yourself do not stay. Settings Shale does not know are kept as they are.
- A line you delete takes its default. The cache settings take `"none"` for "forever" or "no
  limit".
- Text that is not valid settings is not saved: the file stays as it was, the settings in use stay,
  the tab keeps your text and says what is wrong. If you quit Shale while such text is still in
  the tab, it goes to the recovery folder as `settings/config.toml` and the next start tells you
  (as for notes that could not be saved). If another program leaves an error in the
  file, Shale keeps using the settings it has, tells you, and does not write over the file until
  it is valid again.
- Passwords and tokens are not in the file: they are in the system keyring.
- An old `config.json` is turned into `config.toml` the first time this version starts; the old
  file is renamed `config.json.migrated`.

## Global settings (`config.toml`)

### Links and notes

| Key | Default | Where |
|---|---|---|
| `ctrl_to_follow` | off (a plain click follows a link) | Settings → Editor → Links |
| `new_notes_next_to_current` | off (new notes go to the vault root) | Settings → Editor → Links |
| `update_links_on_rename` | on | Settings → Editor → Links |
| `auto_convert_canvas` | off (asks) | Settings → Editor → Drawings; or the tick in the canvas question |
| `file_sort` | `NameAsc` (file name A to Z); also `NameDesc`, `ModifiedDesc`, `ModifiedAsc`, `CreatedDesc`, `CreatedAsc` | the sort icon above the file list |

### Appearance and editor

| Key | Default | Where |
|---|---|---|
| `readable_width` | on | Appearance, "Readable line length" |
| `dark_canvas` | on | Appearance, "Dark drawing canvas" |
| `inline_images` | on | Appearance, "Show embedded images and players inside the editor" |
| `media_play_button_opacity` | 0.6 (range 0 to 1) | Settings → Appearance, "Play button over the artwork of a sound or video"; global config; the 100% and fullscreen buttons over pictures and videos use it too |
| `media_volume` | 0.8 (range 0 to 1) | the volume slider of any sound player; one volume for all players; global config |
| `settings_tab_width` | left out (as wide as the longest tab name; 60 points at least, half of the window at most) | the border between the tab list and the page of the Settings window: drag it to set the width, double-click it to go back to automatic |

### Fonts

One table, `fonts` (`[fonts]` in the file), in the settings file. Each area has a `family` (an installed font family such as
"DejaVu Sans"; left out: Shale's own font) and a `size` in points (8 to 48). Settings → Fonts shows them
(see [[Fonts]]).

| Key | Default | Where |
|---|---|---|
| `fonts.same_everywhere` | on | Settings → Fonts, "Use the same font and size everywhere"; on: `fonts.all` is used for every area and the tables of the single areas rest |
| `fonts.all` | Shale's font, 14 | Fonts, "Font and size" while the checkbox is on; the size is the one of dialogs and menus, the text of the editor is drawn 11 % larger (15.5 against 14), as it always was |
| `fonts.ui` | Shale's font, 14 | Fonts, "Dialogs and menus", with the checkbox off: dialogs, menus, sidebars and everything not listed below |
| `fonts.editor_raw` | Shale's font, 15.5 | Fonts, "Editor: raw markdown": the markdown text in the editor, and the raw lines around the cursor in the preview |
| `fonts.editor_rendered` | Shale's font, 15.5 | Fonts, "Editor: rendered text and reading view": the rendered lines of the preview and the reading view |
| `fonts.mail` | Shale's font, 14 | Fonts, "Mail" |
| `fonts.calendar` | Shale's font, 14 | Fonts, "Calendar" (the calendar view and the calendar of the right sidebar) |
| `fonts.social` | Shale's font, 14 | Fonts, "Feeds" |

Inside each of these tables: `family` and `size`. The old `font_size` of earlier versions became the
size of the two editor areas when you updated.
| `line_numbers` | on | Editor, "Show line numbers in the editor" |
| `date_prefix_new_notes` | on | Editor, "Date prefix for new notes": a note Shale makes (new note, red link, quick switcher, new drawing) is named `YYMMDD_name.md` with today's date; the title line shows the name without the prefix (see [[Editor]]) |
| `numbered_headings` | on | Editor, "Number headings": `#` and deeper show 1, 1.1, 1.2 before their text; a note's front matter `numbered-headings: true` or `false` overrides it for that note (see [[Notes and links]]) |
| `format_toolbar` | on | Settings → Editor → Editing, "Show formatting toolbar above the text of a note" |
| `toolbar_size` | 110 | Editing, "Toolbar button size": the size of the toolbar's buttons in percent, 80 to 200 in steps of 10 |
| `link_preview_width` | 640 (at least 240, at most the window) | Vim, "Link preview size", the first number; also set by dragging the preview's edges |
| `link_preview_height` | 480 (at least 160, at most the window) | Editor, "Link preview size (Shift+K)", the second number; "Reset size" brings back 640 x 480 |
| `font_colours` | red, orange, blue, green, purple (`[[229,62,62], …]`, RGB, up to 8) | Editing, "Font colours": the swatches of the toolbar's font colour palette |
| `highlight_colours` | neon yellow, green, cyan, magenta (RGB, up to 8) | Editing, "Highlight colours": the swatches of the toolbar's highlight palette |
| `live_preview` | off | Editor, "Markdown preview in the editor"; the eye icon of the editor |
| `preview_raw_lines` | 0 (range 0 to 20) | Editor, "Raw lines around the cursor" |
| `table_cell_edit` | on | Editor, "Edit table cells in the grid"; a single table is switched by the button above the top right corner of its grid |
| `vim_mode` | off | Vim, "Vim-style editing" |
| `cursor_shape` | `bar` (`bar`, `bar_after`, `block` or `underline`; another word is `bar`) | Editor, "Cursor while typing": also vim's insert and visual mode |
| `normal_cursor_shape` | `block` (the same four words) | Vim, "Cursor in normal mode"; used only with Vim-style editing |
| `cursor_blink` | on | Editor, "Blinking cursor"; off keeps the cursor steady |
| `esc_cursor` | `stay` (`stay` or `step_back`; another word is `stay`) | Vim, "Esc from insert mode": where the cursor lands when Esc leaves insert mode, on the character after the typing or one back like Vim; used only with Vim-style editing |
| `keybindings` | empty (the defaults of [[Keyboard shortcuts]]) | Settings → Keybindings; only the actions you changed are stored |

### Window and vault

| Key | Default | Where |
|---|---|---|
| `version` | the format version of the file (1) | written by Shale |
| `vault` | none | the vault opened last; Settings → Vault → Vault folder |
| `recent` | empty | the recent vaults of the start screen |
| `show_left` | on | the left sidebar toggle of the ribbon |
| `show_right` | on | the right sidebar toggle of the ribbon |
| `sidebars` | empty (default icons and layout) | dragging icons, see [[Window layout]] |
| `sidebars.ribbon` | empty: default order | drag the icons of the ribbon |
| `sidebars.right` | empty: one view with all panels | drag panel icons into views |
| `sidebars.hidden` | empty | right-click an icon: Remove / Restore |
| `right[].icons`, `right[].active`, `right[].weight` | per view: its icons, the shown icon, its share of the height (0 = equal) | the right sidebar's views |

(`right[].…` are the fields of each entry of `sidebars.right`.)

### Calendar

| Key | Default | Where |
|---|---|---|
| `calendars` | empty; each entry has `id`, `name`, `color`, `enabled`, `sync_minutes` (15 for a new CalDAV calendar) and its source (a local `.ics` file, or a CalDAV URL with user name and password) | Settings → Calendar → Calendars… |
| `week_starts_monday` | on | Settings → Calendar, and the Calendars… dialog |

### Mail ([[Mail]])

| Key | Default | Where |
|---|---|---|
| `mail_accounts` | empty; one entry per account, fields below | Mail view → Accounts… |
| `mail_sync_minutes` | 10 (0 = only with Sync) | Settings → Mail, and Accounts… |
| `mail_mark_read` | on | Settings → Mail, and Accounts…, "Mark as read when opened" |
| `mail_mark_read_secs` | 2 (0 = at once) | Settings → Mail, and Accounts… |
| `mail_undo_secs` | 10 (0 = send at once) | Accounts…, Sending, "Undo send" |
| `mail_encryption_consent` | none (nothing is decrypted); the date you typed the consent sentence | Accounts…, Encryption |

Fields of each entry of `mail_accounts`, all in Accounts… (passwords and secrets are in the
system keyring, never in this file):

| Key | Default |
|---|---|
| `mail_accounts[].id` | made by Shale (letters, digits, `-`, `_`) |
| `mail_accounts[].name` | empty; what the account is called |
| `mail_accounts[].host` | empty (the IMAP server) |
| `mail_accounts[].port` | 993 (143 for STARTTLS) |
| `mail_accounts[].user` | empty |
| `mail_accounts[].auth` | `Password`; or `OAuth2` |
| `mail_accounts[].security` | `ImplicitTls`; or `StartTls` (there is no plain-text choice) |
| `mail_accounts[].oauth_provider` | `Google`; or `Microsoft`, `Custom` |
| `mail_accounts[].oauth_client_id` | empty (Shale ships none, you register your own) |
| `mail_accounts[].oauth_auth_url`, `mail_accounts[].oauth_token_url`, `mail_accounts[].oauth_scope` | empty; only for the Custom provider |
| `mail_accounts[].smtp_host` | empty (sending server) |
| `mail_accounts[].smtp_port` | 465 (587 for STARTTLS) |
| `mail_accounts[].smtp_security` | `ImplicitTls`; or `StartTls` |
| `mail_accounts[].from_name`, `mail_accounts[].from_address` | empty (the address defaults to the user name if it is one) |
| `mail_accounts[].signature` | empty |
| `mail_accounts[].server_stores_sent` | off (on for Gmail-like servers that keep sent mail themselves) |
| `mail_accounts[].user_agent_header` | off (privacy) |
| `mail_accounts[].pgp_key`, `mail_accounts[].smime_key` | empty (fingerprints of your GnuPG keys) |
| `mail_accounts[].sign_by_default` | off |
| `mail_accounts[].preferred_standard` | `OpenPgp`; or `Smime` |

### Feeds ([[Feeds]])

| Key | Default | Where |
|---|---|---|
| `social_accounts` | empty | Feeds → Accounts… (secrets in the keyring) |
| `social_last` | none; the account and feed shown last | set by Shale |
| `social_replies` | off | the tick "Include replies from others" in the Import dialog |
| `social_check_minutes` | 5 (0 = never) | Settings → Feeds, and Feeds → Accounts… |

### LaTeX ([[PDF]])

Settings → LaTeX is the table `[tex]` in the settings file.

| Key | Default | Where |
|---|---|---|
| `tex` | the group of the two keys below | Settings → LaTeX |
| `tex.engine` | `Auto` (the TeX Live that Shale installs when it is there, else the `lualatex` of your computer when it is new enough and has all the packages); `System` (the `lualatex` of your computer); `Managed` (the TeX Live that Shale installs) | Settings → LaTeX, "TeX to use" |
| `tex.repository` | `https://mirror.ctan.org/systems/texlive/tlnet` (the TeX Live repository the install and the update download from) | Settings → LaTeX, "Download address" |

The template sets of the vault ([[PDF]], "LaTeX export: template sets") have a settings file of their own each, `templates/tex/<set>/settings.toml`, which lives in the vault and not in the settings file of Shale:

| Key | Default | Where |
|---|---|---|
| `accent` | `9c` for the TU Darmstadt set, empty for others (the accent colour of the templates) | `settings.toml` of the set, the guided setup |
| `language` | `en` (or `de`) | `settings.toml` of the set, the guided setup |
| `department` | empty (used when a note gives no department) | `settings.toml` of the set, the guided setup (Settings → LaTeX → Set up LaTeX export…) |
| `author` | empty (used for the author field when a note gives none) | `settings.toml` of the set, the guided setup |
| `logo` | none, set by Settings → LaTeX → Template sets → Choose logo… | `settings.toml` of the set |

### Data and troubleshooting ([[Data usage and caches]])

| Key | Default | Where |
|---|---|---|
| `input_log` | off | Settings → Help → Troubleshooting, "Record keyboard input to a file" |
| `caches` | the values below; each can be "forever" or "no limit" | Settings → Data → How long caches are kept; Data usage… rows |
| `caches.picture_days` | 30 (social pictures, days) | same |
| `caches.picture_mb` | 500 (social pictures, MB) | same |
| `caches.post_days` | 365 (social post index, days) | same |
| `caches.posts_per_account` | 10 000 | same |
| `caches.mail_backups` | 3 (newest mail index backups kept) | same |
| `caches.broken_days` | forever (damaged mail index copies) | same |
| `caches.debug_log_mb` | 10 | same |
| `caches.undo_days` | 30 (undo steps of notes, days) | same |

## Vault settings (`<vault>/.shale/settings.json`)

| Key | Default | Where |
|---|---|---|
| `version` | 1 | written by Shale |
| `people_dir` | `Adressbuch` | Settings → Vault → People, "People folder" ([[Tags and people]]) |
| `sync` | the group of the four keys below | Settings → Sync |
| `sync.enabled` | on (only matters when the vault is a git repository) | Settings → Sync, "Sync this vault automatically" ([[Git sync]]) |
| `sync.interval_minutes` | 5 (1 to 1440) | Git sync, "Every … minutes" |
| `sync.commit_prefix` | empty | Git sync, "Commit message prefix" |
| `sync.commit_suffix` | empty | Git sync, "suffix" |
| `sync.lfs_download` | `on_demand` (a media file stored with Git LFS is downloaded when you play or open it; `always`: after every sync) | Git sync, "Download media files after every sync" (shown only when `git-lfs` is installed) |

The git remote itself is git's (`git remote`), set with **Connect** in the same section.

## Vault session state (`<vault>/.shale/state.json`)

Kept by Shale as you work, so a vault opens as you left it ([[Tabs]], [[Import]]).

| Key | Default | Meaning |
|---|---|---|
| `version` | 2 | format version |
| `tabs` | empty | the open tabs in order; each has `tabs[].file` (a note or drawing, relative to the vault) or `tabs[].feed` (a Feeds tab) and `tabs[].pinned` |
| `panes` | empty | the panes side by side, left to right ([[Tabs]]); each has `panes[].tabs` (as `tabs`), `panes[].active` (the open tab, an index into its tabs) and `panes[].weight` (its share of the width, 1); a file without `panes` loads as one pane made of `tabs`. `tabs` is also written as the tabs of the first pane |
| `focused_pane` | 0 | the index of the pane that had the focus |
| `last_import_folder` | empty | the folder of the last import |
| `last_social_folder` | empty (the first save asks for `Social`) | the folder of the last saved post |
| `recent_tags` | empty | tags used for imports, most recent first |
| `latex_export` | `latex` false, `set` and `kind` empty, `setup_seen` false | what the note export dialog had last: `latex` (the LaTeX engine was used), `set` (the folder name of the template set) and `kind` (its id) ([[PDF]], "a note through a template"); `setup_seen` (the guided setup of the LaTeX export has been opened in this vault, so it does not open by itself again: [[PDF]], "the guided setup") |

#userguide #settings
