# Spec: Web clipper, a browser extension that saves pages into the vault (CLIP)

**IS NOW** = true of the code today (⚠ = known defect) · **SHOULD BE** = agreed target.
Format: [ADR-0001](../decisions/0001-spec-driven-development.md). Decisions and rejected alternatives:
[ADR-0036](../decisions/0036-browser-extension-clipper.md). Issue: #427.

Nothing here is built yet: every row is **SHOULD BE** (#427). The code is split into these issues (each flips its rows to IS NOW):
#612 clip format and spool core (CLIP-003 to CLIP-006, CLIP-011, CLIP-032); #613 native host (CLIP-001, CLIP-002, CLIP-007, CLIP-008, host side of CLIP-060 and CLIP-062);
#614 clip target setting (CLIP-057 to CLIP-059, CLIP-062, CLIP-063, CLIP-065); #615 import of spooled clips (CLIP-009, CLIP-010, CLIP-012, CLIP-034 to CLIP-040, CLIP-043 to CLIP-049, CLIP-066 to CLIP-068);
#616 extension core (CLIP-016 to CLIP-031, CLIP-033, CLIP-041, CLIP-042); #617 host manifests (CLIP-013 to CLIP-015); #618 extension settings and first run (CLIP-050 to CLIP-055, CLIP-060 to CLIP-065 extension side);
#619 Paste as Markdown (CLIP-056); #620 packaging (no rows yet); #621 user guide and limits (CLIP-069, CLIP-070). Owner decision 2026-11-22: this round
starts; the code is split into issues that each name their rows. The rows were brought up to date
with `main` on 2026-11-23: the busy registry (JOB-003…JOB-014, CLIP-067), device-local state
(NET-016, CLIP-066), the Import pipeline (IMP-003, IMP-004, IMP-006, CLIP-009, CLIP-012) and the
known vaults (VAULT-029, VAULT-038, CLIP-057). The Test column names the planned
way to test the row: *Rust* is a unit or integration test under `src/` or `tests/`, *Playwright* a
browser test with the extension loaded, run in a podman stack like the other external-behaviour
stacks (`tests/mail-env/`; DEV rule "real servers in podman"), *by hand* a check that no automatic
test can make (a real Chrome or Firefox profile).

## Native host and spool folder

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| CLIP-001 | `shale --native-host` is a mode of the Shale binary that is chosen in `main` before any GUI, window or vault code starts; it reads one native-messaging message from standard input (4-byte length, JSON), writes one reply to standard output and exits. | SHOULD BE (#427) | — | Rust: start the binary with the flag, feed a message, check reply and exit code |
| CLIP-002 | The host checks the caller: it accepts a message only when the browser started it for the extension's fixed ID (Chromium: the `chrome-extension://<id>/` origin argument; Firefox: the extension ID argument); any other or missing caller gets `{ok:false}` and writes nothing. | SHOULD BE (#427) | — | Rust: wrong, missing and right caller arguments |
| CLIP-003 | The host checks shape and size: the message must be JSON of the clip format with a known `version`, and at most 64 MiB (the native-messaging limit); anything else gets `{ok:false}` with a reason and writes nothing. | SHOULD BE (#427) | — | Rust: oversized, truncated, non-JSON and wrong-shape inputs |
| CLIP-004 | A valid message is written as `<uuid>.json` into the spool folder: the file is created with mode 0600 as a temporary name in the same folder and renamed into place, so a reader never sees a half-written file; then the host replies `{ok:true}`. | SHOULD BE (#427) | — | Rust: no `.tmp` left, mode 0600, content equals message |
| CLIP-005 | The spool folder is `clips/` inside Shale's data folder, computed by one function, `clip::spool_dir()`, built on the data folder that the rest of Shale uses (`dirs::data_dir().join("shale")`, as `instance::default_dir`, `recovery::default_root` and `mail/store.rs` do; the host and the importer must not each build their own): Linux `$XDG_DATA_HOME/shale/clips/` (default `~/.local/share/shale/clips/`), macOS `~/Library/Application Support/shale/clips/`, Windows `%APPDATA%\shale\clips\`. On Linux and macOS the folder is created with mode 0700, and the host refuses to write when it is a symlink, owned by another user, or group- or world-writable. On Windows the folder sits in the user's profile and inherits its owner-only ACLs, and the host refuses a reparse point (junction or symlink). The spool is in the data folder, never inside a vault (so a vault in a synced folder never carries it, NET-016, CLIP-066). The host and the importer are the same binary and compute the path with that one function. Note for later, not a rule to build now: if Shale is ever shipped as a Flatpak, the host manifest must point at a wrapper (`flatpak run --command=shale <app-id> --native-host`), and host and app then share the sandbox's data folder (`~/.var/app/<app-id>/data/shale/clips/`); Flatpak browsers stay the limit of CLIP-015. | SHOULD BE (#427) | — | Rust: the host and the importer call one path function, and a test checks both get the same path; Unix: wrong owner, mode, symlink; Windows: reparse point (cross-check build, by hand on Windows) |
| CLIP-006 | The spool holds at most 100 pending files; when it is full the host replies `{ok:false}` with the reason "spool full" and writes nothing, so a runaway extension cannot fill the disk. | SHOULD BE (#427) | — | Rust: 100 files, the 101st is refused |
| CLIP-007 | The host opens no network socket, no listening port and no HTTP server, and needs no pairing or key; it only reads standard input, writes standard output and writes one file. | SHOULD BE (#427) | — | Rust: the host code path has no `std::net` use (a cheap guard greps the module; the module also has no dependency on an HTTP client crate) and runs under a no-network check |
| CLIP-008 | The host works while Shale is closed: it never needs a running Shale, and the clip waits in the spool until Shale starts. | SHOULD BE (#427) | — | Rust: host run without any Shale process, then import at start |
| CLIP-009 | Shale imports the spool files at start and when a new file appears while it runs (a watch on the folder plus a check at start and every few seconds, so a missed event is caught); each file is turned into a note (CLIP-034 to CLIP-049), written into the clip target (CLIP-012), and deleted from the spool only after the note and its images are written. A clip is the person's own gesture in the browser, so it is written at once: no Import dialog opens and the confirmation of IMP-001 does not apply; the new notes are reported with a toast and in the summary of IMP-056 when two or more arrive together. Only one importer runs at a time, and it is one task of the busy registry (CLIP-067). | SHOULD BE (#427) | — | Rust: spool with several files, notes and images exist, spool empty |
| CLIP-010 | A spool file that does not parse, fails the shape or size check again in Shale, or cannot be converted is moved to `clips/quarantine/` (mode 0600) with a message in the app, is never retried, and never blocks other files. | SHOULD BE (#427) | — | Rust: bad files among good ones |
| CLIP-011 | Shale checks the clip again with the same rules as the host (CLIP-003, CLIP-005 file mode and owner): it does not trust a spool file because the host wrote it. | SHOULD BE (#427) | — | Rust: a hand-made file with a wrong mode or shape is quarantined |
| CLIP-012 | An imported clip lands in the clip target (CLIP-057): the clip vault and the clip folder in it, read when Shale imports the clip, not when the clip is made; one note per clip, named `YYYY-MM-DD <page title>` like an imported mail (IMP-003), made safe by the link and portable name rules (VAULT-012, VAULT-026, VAULT-027) and the vault path rules (VAULT-018), and made unique when taken (a second clip of the same page is a second note, not a replacement; IMP-005's duplicate mark is not used). When the clip vault is the open vault, the note and its images are written through it (the index and the open file list see them, DATA-004 atomic writes, one history snapshot as in IMP-072); otherwise the files are written into the vault's folder directly under the same rules and the open vault is untouched. When the clip vault is not a known vault any more, or its folder is missing (VAULT-038), or the clip folder cannot be made, the clip **stays in the spool** (it is not hostile, so it is not quarantined), the app says why once, and the import tries again when the target or the folder changes. | SHOULD BE (#427) | — | Rust: hostile titles (`../`, empty, long) |

## Installing the host

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| CLIP-013 | A Shale setting (Settings → Import, next to the clip target of CLIP-057) installs the native-messaging host manifest for each browser the user ticks (Firefox, Chrome, Chromium, Brave, Edge, Vivaldi and on macOS and Windows their platform locations), names the Shale binary by absolute path, and lists only the extension's fixed ID in `allowed_origins` (Chromium family) or `allowed_extensions` (Firefox). | SHOULD BE (#427) | — | Rust: manifests written to a temporary home for every browser, content checked |
| CLIP-014 | The setting shows per browser whether the manifest is installed and current, and can remove it; removing deletes only the manifest files Shale wrote. | SHOULD BE (#427) | — | Rust and UI test: install, status, remove |
| CLIP-015 | Flatpak browsers cannot reach a host outside their sandbox (Mozilla bug 1955255); the setting says so next to such a browser, does not claim success, and this is a documented limit (CLIP-070). Firefox as a Snap can use native messaging through the XDG WebExtensions portal when the preference `widget.use-xdg-desktop-portal.native-messaging` is on; the setting says how to turn it on. Other Snap browsers are not tested and the setting promises nothing for them. | SHOULD BE (#427) | — | Rust: detection of the Flatpak and Snap profile paths; wording by hand |

## Extension: permissions and entry points

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| CLIP-016 | The extension is Manifest V3 for Firefox and Chromium-based browsers and asks for exactly `activeTab`, `scripting`, `contextMenus` and `nativeMessaging`, and `optional_host_permissions` for all sites; it has no always-on content script and no other permission. | SHOULD BE (#427) | — | Rust or script: a test parses `manifest.json` and compares the permission lists |
| CLIP-017 | The page is read only after a user gesture on the active tab (toolbar popup or context menu click), by a script injected for that one click. | SHOULD BE (#427) | — | Playwright: no injection before the click, one after |
| CLIP-018 | The toolbar popup offers **Save page to vault** and **Save selection to vault**; the first saves what the page shows right now, after its JavaScript has run. | SHOULD BE (#427) | — | Playwright: a page that builds its text by script is saved with that text |
| CLIP-019 | The context menu offers the same two entries ("Save page to vault", "Save selection to vault"). | SHOULD BE (#427) | — | Playwright: Chromium; by hand: Firefox if the harness cannot open the menu |
| CLIP-020 | In the toolbar popup, **Save selection to vault** is greyed out (disabled) when the page has no selection and enabled when it has, in both browsers. | SHOULD BE (#427) | — | Playwright |
| CLIP-021 | In Firefox the context-menu entry "Save selection to vault" is greyed out while nothing is selected (updated from `menus.onShown`, then `menus.refresh`). | SHOULD BE (#427) | — | by hand (Firefox) |
| CLIP-022 | In Chromium-based browsers the context-menu entry "Save selection to vault" uses the `selection` context, so it is hidden (not greyed) when nothing is selected, because Chrome has no `onShown`; this difference is documented in the user guide. | SHOULD BE (#427) | — | Playwright: right click with and without a selection |
| CLIP-023 | After a save the extension shows the result in the browser (saved, or the reason it failed: Shale host missing, host refused, spool full); a save never fails silently. | SHOULD BE (#427) | — | Playwright: success and a host that replies `{ok:false}` |

## Capture (allowlist)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| CLIP-024 | The injected script builds the clip by walking the live DOM, including open shadow roots, and emitting only elements from the allowlist: h1 to h6, p, a, img, ul, ol, li, table and its parts, pre, code, blockquote, em, strong, del, br, hr, plus text nodes; every other element is skipped but its allowed descendants are still walked. | SHOULD BE (#427) | — | Playwright with fixture pages; unit test of the walker in a DOM library |
| CLIP-025 | Only the attributes `href`, `src`, `alt` and `title` are emitted, and only on the elements that use them; no `id`, `class`, `style`, event handler or data attribute is emitted. | SHOULD BE (#427) | — | Playwright: fixture with hostile attributes |
| CLIP-026 | An element is emitted only when it is visible, tested with `checkVisibility({ visibilityProperty: true, opacityProperty: true })` (so not `display:none`, `content-visibility:hidden`, `visibility:hidden` or `opacity:0`); hidden text does not enter the clip. Content that is only scrolled out of view, including content skipped by `content-visibility:auto`, is visible to the page and is captured. | SHOULD BE (#427) | — | Playwright: hidden element fixtures |
| CLIP-027 | The script never emits `input`, `textarea`, `select`, `option`, `button`, `datalist`, `output`, any element with `contenteditable`, or text under such an element, so what is typed into a password or PIN field never leaves the browser, by construction, not by a list of names. | SHOULD BE (#427) | — | Playwright: password, PIN, OTP, textarea, contenteditable and select fixtures; the clip JSON is searched for the typed secret |
| CLIP-028 | The script drops `script`, `style`, `noscript`, `template`, `iframe`, `object`, `embed`, `svg` and `canvas` content and everything in `<head>` except the title; it reads no cookie, storage, header or network response of the page. | SHOULD BE (#427) | — | Playwright |
| CLIP-029 | A **selection** clip re-reads the live selection ranges in the injected script (`getSelection().getRangeAt(i)`), walks only the nodes in the ranges with CLIP-024 to CLIP-028, and drops a range whose boundary lies inside a form field or `contenteditable` element. | SHOULD BE (#427) | — | Playwright: selection across a form and inside a textarea |
| CLIP-030 | The extension never uses the `selectionText` value of the context-menu event (a browser-made string that may hold form-field text); the text comes only from CLIP-029. | SHOULD BE (#427) | — | Playwright: selection inside a password-like field is not in the clip; code review (a test greps the extension source for `selectionText`) |
| CLIP-031 | Cross-origin iframes, canvas content and closed shadow roots are not captured and are not an error; the clip records that content was skipped. | SHOULD BE (#427) | — | Playwright: fixture with an iframe from a second origin |

## Source URL

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| CLIP-032 | The source URL stored in the clip and the note is the page address without its fragment and without query parameters that look like tokens (names such as `token`, `access_token`, `auth`, `key`, `sig`, `session`, `code`, `password`, and the like; list in the code); other query parameters stay. | SHOULD BE (#427) | — | Rust: a table of URLs |
| CLIP-033 | Only the source URL is cleaned: the `href` and `src` values inside the clip body are saved as they are on the page. | SHOULD BE (#427) | — | Rust and Playwright |

## Conversion in Shale

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| CLIP-034 | Shale parses the clip's HTML again and keeps only the same allowlist and attributes as CLIP-024 and CLIP-025; anything else in a spool file (script, form controls, event attributes, unknown elements) is dropped, never written. | SHOULD BE (#427) | — | Rust: hostile spool files |
| CLIP-035 | The allowlisted HTML is converted to Markdown in Rust (reusing `src/social/markup.rs`): headings, paragraphs, links, lists, tables, code blocks, quotes, emphasis, line breaks and rules. | SHOULD BE (#427) | — | Rust: golden input and output |
| CLIP-036 | The note starts with front matter that holds the source URL (CLIP-032), the page title and the capture date and time, and a `type: web-clip`; a selection clip also says `selection: true`. | SHOULD BE (#427) | — | Rust |
| CLIP-037 | A link whose scheme is not `http`, `https` or `mailto` (`javascript:`, `data:`, `file:` and the like) is written as plain text, not as a link. | SHOULD BE (#427) | — | Rust |
| CLIP-038 | Text in the clip is escaped so that it cannot add Markdown or wiki-link syntax of its own beyond what the allowlist produces (no `![[…]]` embed, no front-matter break from page text). | SHOULD BE (#427) | — | Rust: page text with `---`, `![[x]]`, `[[x]]` |
| CLIP-039 | Shale itself never fetches a URL while importing a clip: no image, link card, redirect or video; the importer has no network use. | SHOULD BE (#427) | — | Rust: import under a check that no process or socket is opened (a cheap guard greps the module for `curl` and `net`; the module also has no dependency on an HTTP client crate, checked against `Cargo.toml` and the module's imports) |

## Images

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| CLIP-040 | Image bytes travel from the browser to Shale only as base64 inside the spool file; they are never placed in the note text. | SHOULD BE (#427) | — | Rust |
| CLIP-041 | Without the all-sites permission, the extension reads images that are same-origin, CORS-readable (canvas not tainted), `data:` or `blob:`; every other image stays a link in the note. | SHOULD BE (#427) | — | Playwright: fixtures for each case |
| CLIP-042 | With the permission, the background script fetches only images that were emitted by the allowlist walk, have loaded (`complete` and `naturalWidth > 0`), and whose URL also appears in the page's resource timing entries (`performance.getEntriesByType("resource")`) or is the `currentSrc` of a loaded image; it never fetches any other URL. This is a new request, not a cache read (the HTTP cache is separate per top-level site, so it usually misses), and it carries the user's cookies. | SHOULD BE (#427) | — | Playwright: a hidden `<img>` and a CSS background are never requested |
| CLIP-043 | The size caps are 10 MB per image and 40 MB per clip (before base64); an image over a cap becomes a link. | SHOULD BE (#427) | — | Rust and Playwright |
| CLIP-044 | Shale decodes each image with the `image` crate under limits (maximum bytes, width, height and pixel count, so a decompression bomb is refused) and re-encodes it to PNG or WebP; the original bytes are never written. | SHOULD BE (#427) | — | Rust: bomb, truncated and polyglot files |
| CLIP-045 | Shale never accepts SVG (and no other format the `image` crate does not decode); such an image becomes a link or is dropped with a note in the log. | SHOULD BE (#427) | — | Rust: an SVG with script |
| CLIP-046 | An image file is named `<sha256 of the re-encoded bytes>.<ext>` in the media folder of the clip vault, `z_media/<YYYY>/<MM>/` for the month of the import (IMP-004, the same place as the other imported attachments; the rule of VAULT-027 for names is met by construction); the name never comes from the page, so a path cannot be made from it, and the same picture twice is one file. | SHOULD BE (#427) | — | Rust: hostile `alt` and URL names |
| CLIP-047 | A note embeds a saved image by its SHA-256 name and keeps the page's `alt` text; an image that is only a link keeps its `alt` and its address. | SHOULD BE (#427) | — | Rust |
| CLIP-048 | An image that fails any check is skipped alone: the clip is still saved, with a link in its place. | SHOULD BE (#427) | — | Rust |

## Video

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| CLIP-049 | A `video` element becomes its title (the `title` attribute or the page title), a link to the page or source address, the duration when known, and the poster image (the `poster` attribute, through the normal image path CLIP-041 to CLIP-048); the video file is never fetched or saved, and no media is decoded or re-encoded. | SHOULD BE (#427) | — | Playwright and Rust |

## First-run settings page

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| CLIP-050 | When the extension is installed, it opens its settings page in a new tab. | SHOULD BE (#427) | — | Playwright (`runtime.onInstalled`) |
| CLIP-051 | The page explains what the extension does and how (what is read, what is sent, where it goes), shows both image modes with an example of the output of each, and lists what affects the result (login state, lazy-loaded images, cross-origin content, the known limits CLIP-070). | SHOULD BE (#427) | — | Playwright: the text and the examples are present; the wording by hand |
| CLIP-052 | The user picks the image mode on this page; choosing "all images" requests the optional all-sites host permission from that click (a user gesture), and declining or denying it leaves the mode on "readable images only". | SHOULD BE (#427) | — | Playwright: grant and deny |
| CLIP-053 | The page checks the native host and shows "Shale connected" or a message with how to install the integration in Shale (CLIP-013). | SHOULD BE (#427) | — | Playwright: a host stub that answers and one that is missing |
| CLIP-054 | The extension re-checks the optional permission on every save (the user can revoke it, Firefox lets them in the add-on manager), and falls back to "readable images only" for that save when it is gone. | SHOULD BE (#427) | — | Playwright: revoke, then save |
| CLIP-055 | The settings page can be reopened from the toolbar popup and changes the mode later. | SHOULD BE (#427) | — | Playwright |

## Paste as Markdown

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| CLIP-056 | Shale offers **Paste as Markdown**: it reads the clipboard's `text/html`, runs it through the same allowlist and converter as a clip (CLIP-034 to CLIP-038), and inserts the Markdown at the cursor; it fetches nothing and keeps no images beyond the CLIP-040 to CLIP-048 rules for `data:` images. | SHOULD BE (#427) | — | Rust and UI test with an HTML clipboard |

## Clip target settings

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| CLIP-057 | The clip target is two global settings in `config.toml` (CFG-001), the one file that also holds the known vaults (CFG-075): the **clip vault**, stored as the canonical path of one of the known vaults (VAULT-029; unset means the vault opened last, which is the first entry of that list, and the user guide and a demo vault are never in it), and the **clip folder** inside it (default `Clips`). Both can be changed in the Import tab of Shale's Settings dialog (CFG-020), where the vault is picked from the known vaults by the name of VAULT-031, and the folder name is checked by the vault path rules (VAULT-018). The setting is global and device-local like the known vaults themselves: it is never written to a vault's `.shale/` (CLIP-066). A vault that leaves the known vaults (Close forgets it, VAULT-035) resets the setting to its default. Missing fields take their defaults, so older files load, and a file from a newer Shale is not overwritten (CFG-007). | SHOULD BE (#427) | — | Rust: defaults, old file, bad folder names (`..`, absolute) |
| CLIP-058 | `config.toml` holds a revision number `clipper.rev`; every change of the clip target in Shale (Settings dialog or an applied request, CLIP-062) raises it by one, written with the settings in one atomic write (CFG-002). | SHOULD BE (#427) | — | Rust |
| CLIP-059 | Shale owns the clip target; the extension stores no value of its own for it, only a cached copy that it shows and replaces from every reply of the host (CLIP-060). | SHOULD BE (#427) | — | Playwright: the cache follows the reply; the extension code has no default |
| CLIP-060 | Every reply of the host (to a clip, and to a new `hello` message that the settings page and the popup send) carries the current target (vault display name and folder) and `clipper.rev`; the `hello` reply also carries the known vaults (VAULT-029) as display names (VAULT-031) and ids; an id is a hash of the canonical path, so no folder path is ever sent to the browser. The host only reads `config.toml` and never writes it. | SHOULD BE (#427) | — | Rust: host replies with a seeded config; a test checks the host code never opens the file for writing |
| CLIP-061 | A fresh extension that has never talked to Shale shows "not connected yet" and no target; on first contact it takes Shale's values (the desktop app has priority when the two have never synced). | SHOULD BE (#427) | — | Playwright: no host, then a host stub |
| CLIP-062 | The settings page changes the target by sending `set_target` with a vault (chosen from the `hello` list), a folder and the revision it last saw; the host checks shape and size as for a clip (CLIP-003) and writes a settings request file into the spool under the rules of CLIP-004 and CLIP-005 (uuid name, 0600, temp and rename), and replies `{ok}`; it writes no config. Shale applies the request like a clip, at start or when it sees the file, as the one writer of `config.toml` (CFG-002): only when the base revision equals the current one and the folder passes VAULT-018, then it raises the revision. | SHOULD BE (#427) | — | Rust: matching, stale and invalid requests; Playwright for the page |
| CLIP-063 | A request with a stale revision, an unknown vault or an invalid folder is dropped (not retried, not quarantined as hostile) with a message in the app and changes nothing; a request file that does not parse is quarantined (CLIP-010). | SHOULD BE (#427) | — | Rust |
| CLIP-064 | After `set_target` the extension shows "requested, Shale applies it when it runs"; its next `hello` shows the real value, and a change made in Shale shows there the same way, with no push from Shale. | SHOULD BE (#427) | — | Playwright |
| CLIP-065 | Settings that exist only in the browser, the image mode and its permission (CLIP-052 to CLIP-054), stay in the extension and are not sent to Shale. | SHOULD BE (#427) | — | Playwright: no message carries them |

## Device-local state, the busy registry, the guide

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| CLIP-066 | **Everything the clipper keeps is device-local** (as NET-016): the spool and its quarantine are in the data folder, the clip target is in the global `config.toml`, and nothing of it is written to `<vault>/.shale/`, so a vault in a folder that another program syncs never carries a spool file, a half-imported clip or the setting of another device. The notes and images a clip makes are ordinary vault files and sync like any other. | SHOULD BE (#427) | — | Rust: a vault found synced (SYNC-059) gets the note and the images, and no clip file or `clipper` key appears inside it |
| CLIP-067 | **The import of clips is a task of the busy registry** (JOB-003, JOB-009, [busy-states.md](busy-states.md)): key `clip-import`, text "Importing saved web pages…", started at start-up when the spool holds files and whenever new files appear, one at a time (a second start is refused, JOB-005), run on a worker thread (decoding and re-encoding images must not block the window, JOB-001). It cannot be cancelled (a half-written clip is worse than a late one; the files that were not reached wait in the spool), so it shows in the indicator (JOB-007) and not as a dialog. A panic ends the task and quarantines the file it was on (JOB-002, CLIP-010). Installing the host manifests (CLIP-013) is short and not a task. | SHOULD BE (#427) | — | Rust: the task is listed while files are imported and gone before the result is taken; a second trigger is refused; the busy-states inventory has the row |
| CLIP-068 | The import of clips uses the import pipeline where it fits and adds nothing parallel to it: link-safe and portable names (VAULT-012, VAULT-026), the media folder and atomic writes (IMP-004, DATA-004), front matter written once as the only record of the header (IMP-050), the one-snapshot rule (IMP-072) and the summary window (IMP-056). Only the allowlist, the clip format and the image checks (CLIP-034 to CLIP-048) are new code. | SHOULD BE (#427) | — | Rust: a clip note has the same front-matter shape rules as an imported post (IMP-058) |
| CLIP-069 | **The user guide describes the clipper** (DEV-018, DEV-020): a note "Web clipper" in `docs/userguide` (what it does, installing the extension and the integration, the two image modes, the clip target, what a clip contains and leaves out, the Firefox/Chrome menu difference of CLIP-022, the known limits of CLIP-070), linked from "Start here"; the settings reference lists `clipper` (vault, folder, `rev`) and the settings of the Import tab; screenshots of the settings and of a clip. | SHOULD BE (#427) | — | process: the guide notes exist and the guide's link check passes (DEV-018) |

## Known limits

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| CLIP-070 | The known limits are documented in the user guide and on the settings page: cross-origin iframes, canvas, closed shadow roots, Flatpak browsers (Snap Firefox works through the portal, CLIP-015; other Snap browsers are not tested), and that a custom PIN pad that shows digits as plain text in a `<span>` is saved like any visible text (CLIP-027 excludes form controls, not look-alikes). | SHOULD BE (#427) | — | by hand: the user guide note and the settings page text exist |
