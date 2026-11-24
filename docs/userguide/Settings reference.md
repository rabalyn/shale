Every setting you can change, its default, and where it lives. Back to [[Start here]]; how the
files work is in [[Vault settings]]. Open the Settings window with **Ctrl+,** or the settings
button of the ribbon; it has a tab per domain (Vault, Editor, Appearance, Fonts, Sync, Publishing, Calendar, Mail, Feeds, Import, Data, LaTeX, Updates, Help) and **Keybindings**, and the button
**Open settings file** at the top right (see below). The tab list is as wide as its longest name at any font size; drag its border to change the width, double-click the border to make it automatic again.

Two files hold settings:

- **Global**: `~/.config/shale/config.toml` (this computer, all vaults). On Windows and macOS
  the program's config folder; the settings file tab shows the path.
- **Vault**: `<vault>/.shale/settings.json` (travels with the vault) and
  `<vault>/.shale/state.json` (session state; in a vault found in a cloud-synced or network folder it is
  kept in Shale's data folder instead, see [[Vaults in cloud and network folders]]).

A value of the wrong shape in the file loads as its default; fields Shale does not know are kept.
The "Where" column says which window changes a value. The global file is meant to be edited
too: it is plain text with a description above every setting.

# The Vault and Editor tabs

![[settings-vault.png]]

The **Vault** tab is the short page Settings opens on: the vault folder, the button that closes the vault, the people folder, the bibliography file (the `.bib` file that notes use for citations, see [[Editor#The bibliography of a thesis]]) and the check of file names for Windows and macOS.

![[settings-editor.png]]

The **Editor** tab holds the settings of the editor (cursor, links, drawings, the formatting toolbar and its colours); the tables below say which key each one is.

# Finding a setting

![[settings-find.png]]

The field at the top of the Settings window (**Ctrl+F**, or **/** while no text field has the keyboard)
searches every setting of every tab and the actions of the Keybindings tab. Type a few words: a
setting is found by its name, by its description (the text above it in the settings file) or by the
name of its tab, a key action by its name or by its keys (`ctrl+h`, `F6`). Each result shows its
tab at the right. **↓** and **↑** (also Ctrl+J and Ctrl+K) choose, **Enter** or a click goes there:
the tab is shown, the page scrolls to the setting and a box pulses around it for two seconds. Where
animations are off, the box stands still for the same time. **Esc** closes the list, a second Esc
closes the window. A setting that is not shown at the moment (the font of one area while one font is
used for all) pulses the control that makes it appear instead.

On the Keybindings tab the field of the tab filters the list, see [[Keyboard shortcuts]].

# The settings file

![[settings-file.png]]

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

# Global settings (`config.toml`)

## Links and notes

| Key | Default | Where |
|---|---|---|
| `ctrl_to_follow` | off (a plain click follows a link) | Settings → Editor → Links |
| `new_notes_next_to_current` | off (new notes go to the vault root) | Settings → Editor → Links |
| `update_links_on_rename` | on (links, embeds and property links in other notes follow a rename or move) | Settings → Editor → Links |
| `auto_convert_canvas` | off (asks) | Settings → Editor → Drawings; or the tick in the canvas question |
| `file_sort` | `NameAsc` (file name A to Z); also `NameDesc`, `ModifiedDesc`, `ModifiedAsc`, `CreatedDesc`, `CreatedAsc` | the sort icon above the file list |

## Appearance and editor

| Key | Default | Where |
|---|---|---|
| `theme` | the table of the four keys below | The colours of the window: Appearance, "Theme" (see [[Themes]]) |
| `theme.name` | `"Shale"` | Appearance, "Theme": the theme by name, one of the built-in themes or a file of the themes folder (see [[Themes]]); an unknown name gives Shale |
| `theme.follow_system` | off | Appearance, "Follow the system's light or dark mode": with it on, `theme.light` or `theme.dark` is used instead of `theme.name`, by the mode of the system |
| `theme.light` | `"Catppuccin Latte"` | Appearance, "Light theme" (with follow system on) |
| `theme.dark` | `"Shale"` | Appearance, "Dark theme" (with follow system on) |
| `readable_width` | on | Appearance, "Readable line length" |
| `dark_canvas` | on | Appearance, "Dark drawing canvas" |
| `inline_images` | on | Appearance, "Show embedded images and players inside the editor" |
| `media_play_button_opacity` | 0.6 (range 0 to 1) | Settings → Appearance, "Play button over the artwork of a sound or video"; global config; the 100% and fullscreen buttons over pictures and videos use it too |
| `media_volume` | 0.8 (range 0 to 1) | the volume slider of any sound player; one volume for all players; global config |
| `media_libraries` | none (a list of `[[media_libraries]]` tables with `name` and `path`) | the library folders of this computer, for links like `![[@Videos/a.mp4]]` to media outside the vault; global config, not in the vault because the folder differs from computer to computer; Settings → Import, "Library folders" (a name and a folder per line, Add library, Remove); see [[Sounds and videos]] |
| `settings_tab_width` | left out (as wide as the longest tab name; 60 points at least, half of the window at most) | the border between the tab list and the page of the Settings window: drag it to set the width, double-click it to go back to automatic |
| `left_sidebar_width` | left out (260 points; 170 at least, 520 at most) | the border between the left sidebar and the notes: drag it to set the width; it is kept after a restart; "Reset layout" in the ribbon's menu goes back to the default |
| `right_sidebar_width` | left out (290 points; 200 at least, 560 at most) | the border between the notes and the right sidebar: drag it to set the width; it is kept after a restart; "Reset layout" goes back to the default |

## Fonts

One table, `fonts` (`[fonts]` in the file), in the settings file. Each area has a `family` (an installed font family such as
"DejaVu Sans"; left out: Shale's own font) and a `size` in points (8 to 48). Settings → Fonts shows them
(see [[Fonts]]). There is no setting for the list of fonts: it draws each entry in its own font,
loading the entries on screen once you stop scrolling.

| Key | Default | Where |
|---|---|---|
| `fonts.same_everywhere` | on | Settings → Fonts, "Use the same font and size everywhere"; on: `fonts.all` is used for every area and the tables of the single areas rest |
| `fonts.all` | Shale's font, 14 | Fonts, "Font and size" while the checkbox is on; the size is the one of dialogs and menus, the text of the editor is drawn 11 % larger (15.5 against 14), as it always was |
| `fonts.ui` | Shale's font, 14 | Fonts, "Dialogs and menus", with the checkbox off: dialogs, menus, sidebars and everything not listed below |
| `fonts.editor_raw` | Shale's font, 15.5 | Fonts, "Editor: raw markdown": the markdown text in the editor, and the raw lines around the cursor in the preview |
| `fonts.editor_rendered` | Shale's font, 15.5 | Fonts, "Editor: rendered text and reading view": the rendered lines of the preview and the reading view |
| `fonts.mail` | Shale's font, 14 | Fonts, "Mail" |
| `fonts.calendar` | Shale's font, 14 | Fonts, "Calendar" (the calendar view, the calendar of the right sidebar and the Calendar pane) |
| `fonts.social` | Shale's font, 14 | Fonts, "Feeds" (the Feeds tab and the Feeds pane of the right sidebar) |

Inside each of these tables: `family` and `size`. The old `font_size` of earlier versions became the
size of the two editor areas when you updated.
| `line_numbers` | on | Editor, "Show line numbers in the editor" |
| `date_prefix_new_notes` | on | Editor, "Date prefix for new notes": a note Shale makes (new note, red link, quick switcher, new drawing) is named `YYMMDD_name.md` with today's date; the title line shows the name without the prefix (see [[Editor]]) |
| `properties_in_editor` | Folded | Appearance, "Properties in the editor": how the properties (the lines between the two `---` at the top of a note) show: `Folded` (one line), `Card` or `Raw` (all lines). In the editor `Folded` and `Card` start a note with its properties folded, `Folded` to a summary line and `Card` to the card of the reading view, `Raw` opens it unfolded (see [[Editor]]) |
| `properties_in_reader` | Card | Appearance, "Properties in the reading view": the same choice for the reading view; a card is made for the type of the note (see [[Reading view]]) |
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
| `keybindings` | empty (the defaults of [[Keyboard shortcuts]]; macOS has other defaults for some actions) | Settings → Keybindings; only the actions you changed are stored |

## Window and vault

| Key | Default | Where |
|---|---|---|
| `version` | the format version of the file (3) | written by Shale |
| `vault` | none | the vault opened last; Settings → Vault → Vault folder |
| `vaults` | empty | the known vaults, the one opened last first: a `[[vaults]]` table each, with `name` and `path`; the start screen lists them. Shale adds a vault when it has opened it. An empty `name` becomes the folder's name. Replaces the old `recent` list, which is turned into `vaults` at the first start |
| `vaults.name` | the folder's name | what the vault is called in the list and in the window title; only on this computer, never in the vault |
| `vaults.path` | none | the folder of the vault, with symbolic links resolved |
| `show_left` | on | the left sidebar toggle of the ribbon |
| `show_right` | on | the right sidebar toggle of the ribbon |
| `sidebars` | empty (default icons and layout) | dragging icons, see [[Window layout]] |
| `sidebars.ribbon` | empty: default order | drag the icons of the ribbon; `files`, `search`, `tags`, `mail`, `calendar` and `social` in this list and in `sidebars.right` at once are a copy (see [[Window layout]]) |
| `sidebars.right` | empty: one view with all panels | drag panel icons, and Files, Search, Tags, Mail, Calendar and Feeds (`files`, `search`, `tags`, `mail`, `calendar`, `social`), into views |
| `sidebars.hidden` | empty | right-click an icon: Remove / Restore |
| `right[].icons`, `right[].active`, `right[].weight` | per view: its icons, the shown icon, its share of the height (0 = equal) | the right sidebar's views |

(`right[].…` are the fields of each entry of `sidebars.right`.)

## Calendar

| Key | Default | Where |
|---|---|---|
| `calendars` | empty; each entry has `id`, `name`, `color`, `enabled`, `sync_minutes` (15 for a new CalDAV calendar) and its source (a local `.ics` file, or a CalDAV URL with user name and password) | Settings → Calendar → Calendars… |
| `week_starts_monday` | on | Settings → Calendar, and the Calendars… dialog |

## Mail ([[Mail]])

| Key | Default | Where |
|---|---|---|
| `mail_accounts` | empty; one entry per account, fields below | Mail view → Accounts… |
| `mail_sync_minutes` | 10 (0 = only with Sync) | Settings → Mail, and Accounts… |
| `mail_mark_read` | on | Settings → Mail, and Accounts…, "Mark as read when opened" |
| `mail_mark_read_secs` | 2 (0 = at once) | Settings → Mail, and Accounts… |
| `mail_undo_secs` | 10 (0 = send at once) | Accounts…, Sending, "Undo send" |
| `mail_encryption_consent` | none (nothing is decrypted); the date you typed the consent sentence | Accounts…, Encryption |

How long GnuPG keeps a passphrase is not a Shale setting: it is `default-cache-ttl` (default 600 seconds) and `max-cache-ttl` (default 7200) in GnuPG's `gpg-agent.conf`. Accounts…, Encryption, "Keeping the key unlocked" shows them and can change them with `gpgconf` (see Mail, "Keep the key unlocked").

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

## Feeds ([[Feeds]])

| Key | Default | Where |
|---|---|---|
| `social_accounts` | empty | Feeds → Accounts… (secrets in the keyring; a Mastodon account also keeps `scopes`, the permissions its token was given: "read write" lets Shale like, boost and post) |
| `social_last` | none; the account and feed shown last | set by Shale |
| `social_replies` | off | the tick "Include replies from others" in the Import dialog |
| `social_check_minutes` | 5 (0 = never) | Settings → Feeds, and Feeds → Accounts… |
| `social_accounts[].post_language` | the language of the account's last post | kept by Shale: the next post of the account starts with it |
| `social_post_language` | the language of the system's locale ("en" without one); "" = none | Settings → Feeds → Post language: the language a new post in the feed view starts with (two-letter code) |

## Search ([[Search and pickers]])

![[settings-search.png]]

| Key | Default | Where |
|---|---|---|
| `search` | the group of the six keys below | Settings → Search |
| `search.prefix_notes` | `f` (one letter: `f:banana` looks in notes only) | Settings → Search, "Prefixes", Notes |
| `search.prefix_mail` | `m` (one letter: `m:banana` looks in mail only) | Settings → Search, "Prefixes", Mail |
| `search.prefix_posts` | `s` (one letter: `s:banana` looks in posts only) | Settings → Search, "Prefixes", Posts |
| `search.hit_display` | `follow` (`follow`, `raw` or `rendered`: how the lines of notes that a search found are drawn; `follow` is rendered while the Markdown preview is on or the open note is in the reading view, raw otherwise; any other word reads as `follow`) | Settings → Search, "Hits of notes" |
| `search.advanced` | `false` (`true` shows the query as text instead of rows in the gear filter form of the search; set by the **Advanced** switch of the form, the same in the sidebar, the picker and the boards; a query that does not fit the rows is shown as text whatever this says) | the **Advanced** switch of the filter form (gear next to the search box) |
| `search.sources` | `["notes", "mail", "posts"]` (the sources a query without a prefix looks in and lists; an empty or unknown list means all three) | Settings → Search, "Sources", and Shift+click on a source button above the results |

The three letters must differ, and no combination of them may spell a field of the mail or post search (`to`, `in`, …); letters Shale
cannot use are shown with the reason in Settings → Search, and read as `f`, `m`, `s` when they are in the file.

## Import ([[Import]])

| Key | Default | Where |
|---|---|---|
| `outside_files` | `"Ask"` (`"Copy"`, `"Link"`) | Settings → Import, "Files from outside the vault": what a `[!` pick, a paste or an import does with a picture, PDF, sound or video that is not in the vault: ask, copy it into `z_media/` or link it where it is; the start value of the Copy / Link choice in the Import dialog (Ask starts at Copy); other files are always copied; global config; see [[Sounds and videos]] |
| `docx_import` | off | Settings → Import, "Import Word files as notes (needs pandoc)"; global config |
| `import_header_list` | off | Settings → Import, "Header list in the note body" (e-mails, events and saved posts); the start value of the box of the same name in the Import dialog |
| `import_skip_signatures` | on | Settings → Import, "Skip signatures": the start value of the box of the same name in the Import dialog; the signature files of signed mail and the pictures of signature blocks start unticked, and the signature text of the mail and of the mails it quotes is cut |
| `import_leave_out_quoted` | off | Settings → Import, "Leave out quoted mails": the start value of the box of the same name in the Import dialog; an imported e-mail keeps only the text its sender wrote, not the earlier mails it quotes |
| `pandoc_path` | empty (search `PATH`, then `/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin`, and on Windows `%LOCALAPPDATA%\Pandoc` and `%ProgramFiles%\Pandoc`) | Settings → Import, "pandoc program" (the Word export looks for pandoc the same way) |

To set it up: install pandoc (Linux `sudo pacman -S pandoc-cli` or `sudo apt install pandoc`, macOS `brew install pandoc`,
Windows `winget install JohnMacFarlane.Pandoc`), tick the box and press Check again. The export of a note to Word (the note's ⋯ menu) needs pandoc too, but not the box.

## LaTeX ([[PDF]])

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
| `department_logo` | none, set by Settings → LaTeX → Template sets → Choose department logo… (cleared by Remove department logo); the logo of your department, shown under the TU logo | `settings.toml` of the set, the guided setup (logo step) |

## Updates ([[Updates]])

Settings → Updates is the table `[updates]` in the settings file. `SHALE_UPDATES=off` in the environment switches updates off whatever is set here (no request, no notice).

| Key | Default | Where |
|---|---|---|
| `updates` | the group of the four keys below | Settings → Updates |
| `updates.mode` | `Ask` (not chosen yet: the first update found asks); `Off` (no request, no notice), `Notify`, `Download` or `Install` | Settings → Updates, "When an update is found"; or the question when the first update is found |
| `updates.skipped_version` | empty | the notice's "Skip this version" |
| `updates.last_check` | `0` (never; seconds since 1970 of the last successful check) | written by Shale after a check; shown in Settings → Updates |
| `updates.install_on_start` | empty (a downloaded version to install at the next start) | the notice's "At next start" |

## Data and troubleshooting ([[Data usage and caches]])

| Key | Default | Where |
|---|---|---|
| `input_log` | off | Settings → Help → Troubleshooting, "Record keyboard input to a file" |
| `caches` | the values below; each can be "forever" or "no limit" | Settings → Data → How long caches are kept; Data usage… rows |
| `caches.picture_days` | 30 (social pictures and videos, days) | same |
| `caches.picture_mb` | 500 (social pictures and videos, MB) | same |
| `caches.post_days` | 365 (social post index, days) | same |
| `caches.posts_per_account` | 10 000 | same |
| `caches.mail_backups` | 3 (newest mail index backups kept) | same |
| `caches.broken_days` | forever (damaged mail index copies) | same |
| `caches.debug_log_mb` | 10 | same |
| `caches.undo_days` | 30 (undo steps of notes, days) | same |

# Properties of a note

Some behaviour is decided by a note's own properties (front matter) instead of a setting; they are not stored in any settings file.
`numbered-headings` (`true` or `false`) overrides the setting `numbered_headings` for the note ([[Notes and links]]). The PDF export reads
`print-paper`, `print-margins`, `print-font`, `print-font-size`, `print-line-spacing`, `print-paragraph-spacing`, `print-align`,
`print-heading-color`, `print-heading-weight`, `print-heading-sizes`, `print-break-before`, `print-title`, `print-toc` and `print-toc-page-break` (any of them also switches on the plain paper look of the PDF); each has its values, default and effect in the table of
[[PDF#Print properties of a note]]. Every one is optional, and a value that cannot be read is ignored.
`bibliography` names the `.bib` file of the note's citations, or several as a list; it wins over the vault's bibliography file ([[Editor#The bibliography of a thesis]]).
Shale itself writes `created` and `modified` (full ISO 8601 times) into every Markdown note it saves, and reads an optional `priority` (`highest`, `high`, `medium`, `low`, `lowest`); there is no setting for them (see [[Editor#Created, modified and priority]]).

# Vault settings (`<vault>/.shale/settings.json`)

| Key | Default | Where |
|---|---|---|
| `version` | 1 | written by Shale |
| `people_dir` | `Adressbuch` | Settings → Vault → People, "People folder" ([[Tags and people]]) |
| `bibliography` | empty (not in the file while empty; the `.bib` file, as a path in the vault, a file name or `[[refs.bib]]`, that a note without its own `bibliography:` property uses) | Settings → Vault → Bibliography, "Bibliography file" ([[Editor#The bibliography of a thesis]]) |
| `sync` | the group of the four keys below | Settings → Sync |
| `sync.enabled` | on (only matters when the vault is a git repository; a folder with its own `.git` asks first when it is opened, "Don't sync" sets it off) | Settings → Sync, "Sync this vault automatically" ([[Git sync]]) |
| `sync.interval_minutes` | 5 (1 to 1440) | Git sync, "Every … minutes" |
| `sync.commit_prefix` | empty | Git sync, "Commit message prefix" |
| `sync.commit_suffix` | empty | Git sync, "suffix" |
| `sync.lfs_download` | `on_demand` (a media file stored with Git LFS is downloaded when you play or open it; `always`: after every sync) | Git sync, "Download media files after every sync" (shown only when `git-lfs` is installed) |
| `publish` | the group of the keys below; it is not in the file while all of them are default | Settings → Publishing ([[Publishing a vault]]) |
| `publish.enabled` | off (nothing is published; `shale export-site` writes only the hello-world page) | Settings → Publishing, "Publish this vault as a website" |
| `publish.all_notes` | off (on: every note but the private types) | Settings → Publishing, "All notes" |
| `publish.include_folders`, `publish.include_tags` | empty lists (the notes in these folders, the notes with these tags) | Settings → Publishing, "Publish the notes in these folders" and "… with these tags" |
| `publish.exclude_folders`, `publish.exclude_tags` | empty lists (never published, they win over the include rules) | Settings → Publishing, "Never publish the notes in these folders" and "… with these tags" |
| `publish.private_types` | empty (`email`, `event`, `post`, `person` notes stay out of "all notes" unless named here) | Settings → Publishing, "Also publish" |
| `publish.url`, `publish.branch` | empty (the repository the site is pushed to, and its branch, `main` when empty; an address with a password is not kept) | Settings → Publishing, "Address of the publish repository" and "Branch" |

Settings → Sync starts with **Turn on sync…** ([[Turn on sync]]), which sets up the history, a folder another app syncs, or a git server; the
address field and **Connect** are under **Advanced**. The wizard adds no setting of its own: it turns `history.enabled` on, and for a server it
sets the vault's `origin` and `sync.enabled`. A token is kept in the system keyring, never in a settings file. Where the vault is (a network
folder or a cloud-synced folder) is a warning, not a setting.

The settings of the git that Shale runs are in the settings file of this computer ([[Git sync]]):

| Setting | Default | Where |
|---|---|---|
| `git` | the group of the two keys below | Settings → Sync |
| `git.choice` | `Auto` (the git of this computer when there is one, else Shale's own; `System`: only the git of `PATH`; `Managed`: Shale's own, where Shale installs only git-lfs the git of `PATH` with that git-lfs) | Settings → Sync, "Git to use" |
| `git.updates` | `Off` (no request; `Notify`: look 30 seconds after the start and weekly and say so; `Install`: and install it) | Settings → Sync, "New versions" (shown once Shale's git is installed) |

The file history ([[File history]]) is set in the settings file of this computer:

| Setting | Default | Where |
|---|---|---|
| `history` | the group of the four keys below | Settings → Sync, "File history" |
| `history.enabled` | on (`false`: no new versions are kept; the old ones stay) | Settings → Sync, "Keep earlier versions of my notes" |
| `history.coalesce_minutes` | 10 (1 to 1440; at most one version of a note in this time while you type) | Settings → Sync, "At most one version of a note every … minutes" |
| `history.idle_seconds` | 60 (5 to 3600; a version when a note had no change for so long) | Settings → Sync, "… no change for … seconds" |
| `history.show_panel` | on (`false`: the history column beside a note is hidden; the History panel of the right sidebar stays) | the **×** of the column, **Show it here** in the panel, ⋯ → "History of this note" |

The git remote itself is git's (`git remote`), set with **Connect** in the same section.

Settings → Vault also has **Check names for Windows and macOS…** (no setting, a tool): it lists the files
and folders whose names would break on Windows or macOS and renames them on request, and in the same run lists
the links to media outside the vault ([[Vault settings]]).

# Vault session state (`<vault>/.shale/state.json`)

In a vault that Shale finds in a cloud-synced or network folder, this file (and the undo histories) is in
Shale's data folder, under the vault's ID, so each computer keeps its own.

Kept by Shale as you work, so a vault opens as you left it ([[Tabs]], [[Import]]).

| Key | Default | Meaning |
|---|---|---|
| `version` | 2 | format version |
| `tabs` | empty | the open tabs in order; each has `tabs[].file` (a note or drawing, relative to the vault) or `tabs[].feed` (a Feeds tab) and `tabs[].pinned`; a note tab also has `tabs[].cursor` (the selection as two character positions, the moving end first), `tabs[].scroll` (the scroll offset in points) and `tabs[].reading` (true in the reading view), all left out when not set; a tab of a view without a file has `tabs[].view` instead (`mail`, `calendar`, `tasks`, `graph`, `boards` or `settings_file`) and, where there is something to keep, `tabs[].mail` (account, folder and the `uid` of the open message), `tabs[].calendar` (`mode` and `day`), `tabs[].tasks` (`query`, `show_done`, `only_due`) `tabs[].graph` (`hide_missing`, `hide_orphans`), the last two only when they are not the default, or `tabs[].board` (the board note the Boards view showed, relative to the vault) |
| `panes` | empty | the panes side by side, left to right ([[Tabs]]); each has `panes[].tabs` (as `tabs`), `panes[].active` (the open tab, an index into its tabs) and `panes[].weight` (its share of the width, 1); a file without `panes` loads as one pane made of `tabs`. `tabs` is also written as the tabs of the first pane |
| `focused_pane` | 0 | the index of the pane that had the focus |
| `highlight_colour`, `font_colour` | not written until a colour is picked (neon yellow `#FFFF00`, red `#E53E3E`) | the current colour of the highlight and font colour buttons of the toolbar as `#RRGGBB` ([[Editor]]); an unreadable value gives the default |
| `folds` | empty | the folded blocks of your notes ([[Editor]], "Folding"): a note's path relative to the vault, each with its folds (`kind`, `title`, `nth`, `line`: what the block is and where its header was); a note with no entry has the folds it starts with, an empty list is a note whose folds were all opened; the entry of a note that is gone is dropped |
| `last_import_folder` | empty | the folder of the last import |
| `last_social_folder` | empty (the first save asks for `Social`) | the folder of the last saved post |
| `recent_tags` | empty | tags used for imports, most recent first |
| `latex_export` | `latex` false, `set` and `kind` empty, `setup_seen` false | what the note export dialog had last: `latex` (the LaTeX engine was used), `set` (the folder name of the template set) and `kind` (its id) ([[PDF]], "a note through a template"); `setup_seen` (the guided setup of the LaTeX export has been opened in this vault, so it does not open by itself again: [[PDF]], "the guided setup") |

#userguide #settings
