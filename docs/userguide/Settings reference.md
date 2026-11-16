# Settings reference

Every setting you can change, its default, and where it lives. Back to [[Start here]]; how the
files work is in [[Vault settings]]. Open the Settings window with **Ctrl+,** or the settings
button of the ribbon; it has two tabs, **General** and **Keybindings**.

Two files hold settings:

- **Global**: `~/.config/shale/config.json` (this computer, all vaults). On Windows and macOS
  the program's config folder; Shale tells you the path in the window.
- **Vault**: `<vault>/.shale/settings.json` (travels with the vault) and
  `<vault>/.shale/state.json` (session state).

A value of the wrong shape in the file loads as its default; fields Shale does not know are kept.
You normally never edit these files; the "Where" column says which window changes a value.

## Global settings (`config.json`)

### Links and notes

| Key | Default | Where |
|---|---|---|
| `ctrl_to_follow` | off (a plain click follows a link) | Settings → General → Links |
| `new_notes_next_to_current` | off (new notes go to the vault root) | Settings → General → Links |
| `update_links_on_rename` | on | Settings → General → Links |
| `auto_convert_canvas` | off (asks) | Settings → General → Drawings; or the tick in the canvas question |
| `file_sort` | `NameAsc` (file name A to Z); also `NameDesc`, `ModifiedDesc`, `ModifiedAsc`, `CreatedDesc`, `CreatedAsc` | the sort icon above the file list |

### Appearance and editor

| Key | Default | Where |
|---|---|---|
| `font_size` | 15.5 (range 12 to 24) | Settings → General → Appearance, "Text size" |
| `readable_width` | on | Appearance, "Readable line length" |
| `dark_canvas` | on | Appearance, "Dark drawing canvas" |
| `inline_images` | on | Appearance, "Show embedded images and players inside the editor" |
| `media_play_button_opacity` | 0.6 (range 0 to 1) | Settings → General → Appearance, "Play button over the artwork of a sound or video"; global config; the 100% and fullscreen buttons over pictures and videos use it too |
| `media_volume` | 0.8 (range 0 to 1) | the volume slider of any sound player; one volume for all players; global config |
| `line_numbers` | on | Appearance, "Show line numbers in the editor" |
| `format_toolbar` | on | Settings → General → Editing, "Show formatting toolbar above the text of a note" |
| `toolbar_size` | 120 | Editing, "Toolbar button size": the size of the toolbar's buttons in percent, 80 to 200 in steps of 10 |
| `font_colours` | red, orange, blue, green, purple (`[[229,62,62], …]`, RGB, up to 8) | Editing, "Font colours": the swatches of the toolbar's font colour palette |
| `highlight_colours` | neon yellow, green, cyan, magenta (RGB, up to 8) | Editing, "Highlight colours": the swatches of the toolbar's highlight palette |
| `live_preview` | off | Appearance, "Markdown preview in the editor"; the eye icon of the editor |
| `preview_raw_lines` | 0 (range 0 to 20) | Appearance, "Raw lines around the cursor" |
| `table_cell_edit` | on | Appearance, "Edit table cells in the grid"; a single table is switched by the button above the top right corner of its grid |
| `vim_mode` | off | Appearance, "Vim-style editing" |
| `keybindings` | empty (the defaults of [[Keyboard shortcuts]]) | Settings → Keybindings; only the actions you changed are stored |

### Window and vault

| Key | Default | Where |
|---|---|---|
| `version` | the format version of the file (1) | written by Shale |
| `vault` | none | the vault opened last; Settings → General → Vault folder |
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
| `calendars` | empty; each entry has `id`, `name`, `color`, `enabled`, `sync_minutes` (15 for a new CalDAV calendar) and its source (a local `.ics` file, or a CalDAV URL with user name and password) | Settings → General → Calendars… |
| `week_starts_monday` | on | the Calendars… dialog |

### Mail ([[Mail]])

| Key | Default | Where |
|---|---|---|
| `mail_accounts` | empty; one entry per account, fields below | Mail view → Accounts… |
| `mail_sync_minutes` | 10 (0 = only with Sync) | Accounts… |
| `mail_mark_read` | on | Accounts…, "Mark as read when opened" |
| `mail_mark_read_secs` | 2 (0 = at once) | Accounts… |
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
| `social_check_minutes` | 5 (0 = never) | Feeds → Accounts… |

### Data and troubleshooting ([[Data usage and caches]])

| Key | Default | Where |
|---|---|---|
| `input_log` | off | Settings → General → Troubleshooting, "Record keyboard input to a file" |
| `caches` | the values below; each can be "forever" or "no limit" | Settings → General → How long caches are kept; Data usage… rows |
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
| `people_dir` | `Adressbuch` | Settings → General → People, "People folder" ([[Tags and people]]) |
| `sync` | the group of the four keys below | Settings → General → Git sync |
| `sync.enabled` | on (only matters when the vault is a git repository) | Settings → General → Git sync, "Sync this vault automatically" ([[Git sync]]) |
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
| `last_import_folder` | empty | the folder of the last import |
| `last_social_folder` | empty (the first save asks for `Social`) | the folder of the last saved post |
| `recent_tags` | empty | tags used for imports, most recent first |

#userguide #settings
