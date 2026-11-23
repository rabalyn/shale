# ADR-0036: A companion browser extension saves web pages into the vault

- **Status**: Proposed
- **Date**: 2026-11-19 (revived and brought up to date with `main` 2026-11-23; numbered 0036 because 0023 was taken meanwhile)
- **Specs**: CLIP-001…CLIP-070 ([`docs/spec/web-clipper.md`](../spec/web-clipper.md)), CFG-001, CFG-002, CFG-075, VAULT-018, VAULT-029, NET-016, JOB-003, IMP-003, IMP-004, SEC-001
- **Issues**: #427; split into #612, #613, #614, #615, #616, #617, #618, #619, #620, #621

## Context

The owner wants to save real websites into the vault: the page as the user sees it right now, or
just the selection, as Markdown. A real page needs JavaScript to render, and often a login. Shale
has a page reader (`src/link_preview.rs`) that runs no JavaScript, so it cannot do this. Embedding
a browser engine in Shale is ruled out by the owner (no Electron, no CEF).

Requirements, from the owner:

- Two use cases: save the whole page, save the selection; both from a toolbar button and the
  right-click menu; "save selection" greyed out when nothing is selected.
- Passwords and PINs must never leave the browser.
- Images should end up in the vault, but the owner doubts that downloading arbitrary URLs into the
  file system is safe.
- The solution should be the simplest, most secure and most solid one.

## Decision

1. **A companion extension** (Manifest V3, Firefox and Chromium-based browsers) uses the browser
   the user already has, with its rendered JavaScript and its sign-ins.
2. **Transport: native messaging into a spool folder.** The browser starts `shale --native-host`,
   a mode that branches off before any GUI code and only for the extension's fixed ID. The host
   checks caller, shape and size, writes `<uuid>.json` (0600, temp file and rename) into a 0700
   spool folder, replies `{ok}` and exits. Shale imports and deletes the file, or quarantines a bad
   one; this works while Shale is closed. No socket, no localhost server, no encryption, no
   pairing (CLIP-001 to CLIP-012). A Shale setting installs the host manifests per browser;
   Flatpak browsers are a documented limit; Firefox as a Snap works through the XDG WebExtensions portal (CLIP-013 to CLIP-015).
3. **Permissions are minimal**: `activeTab`, `scripting`, `contextMenus`, `nativeMessaging`, and an
   optional all-sites host permission that is used for images only. No always-on content script:
   the page is read only after a click (CLIP-016, CLIP-017).
4. **Capture by allowlist, in the browser.** The injected script walks the live DOM including open
   shadow roots and emits only visible elements of a short list (headings, paragraphs, links,
   images, lists, tables, code, quotes, emphasis, breaks, rules) with only `href`, `src`, `alt` and
   `title`. Form controls and `contenteditable` content are never emitted, so a password or PIN
   typed into a field cannot leave the browser, by construction and not by guessing field names.
   For a selection the script re-reads the live ranges and drops ranges in form fields; the
   browser's `selectionText` is never used (CLIP-024 to CLIP-031).
5. **The source URL is cleaned, nothing else**: no fragment and no token-like query parameters;
   links inside the clip stay as the page has them (CLIP-032, CLIP-033).
6. **Conversion in Shale**: the same allowlist is applied again to the spool file (the spool is
   input from outside, not trusted), then HTML becomes Markdown in Rust (reusing
   `src/social/markup.rs`) with front matter for source and date (CLIP-034 to CLIP-039).
7. **Images come from the browser, never from Shale.** The bytes travel as base64 inside the
   spool file only. The user chooses the mode on the extension's first-run settings page:
   - *Readable images only* (no extra permission): same-origin, CORS-readable, `data:` and `blob:`
     images; the others stay links.
   - *All images* (optional all-sites permission, requested from the click on that page): the
     background script re-fetches only images that loaded and survived the allowlist, with the
     user's cookies. The permission is re-checked on every save.

   Caps are 10 MB per image and 40 MB per clip. Shale decodes with limits, re-encodes to PNG or
   WebP, names the file by the SHA-256 of the result and never accepts SVG. Shale itself never
   fetches a URL (CLIP-039 to CLIP-048, CLIP-050 to CLIP-055). Image files go to the media
   folder of the vault, `z_media/<YYYY>/<MM>/`, as every other import attachment (IMP-004).
8. **Video is a link**: title, link, duration and poster image; never the file (CLIP-049).
9. **Extra**: "Paste as Markdown" from the clipboard's `text/html` uses the same converter
   (CLIP-056).
10. **App-owned target settings** (CLIP-057 to CLIP-065): the clip vault (one of the known vaults,
    VAULT-029, stored by its canonical path; the extension only ever sees a display name and an id
    that is a hash of the path) and the clip folder are global settings in `config.toml`, owned by
    Shale, which is the only writer. Every host reply carries
    the target and a revision `clipper.rev`; the extension keeps only a cached copy. A change from
    the extension is a `set_target` request file in the spool, carrying the revision the
    extension last saw; Shale applies it only when that equals the current revision, checks the
    folder, then raises the revision, and drops a stale or invalid request with a message.
    Rejected: the host writing `config.toml` itself, which would race with the running app's
    atomic writes and make a second writer.
11. **Device-local, on the busy registry, on the import pipeline** (CLIP-066 to CLIP-069). The spool,
    the quarantine and the clip target live in the data and config folders, never in a vault, so a
    synced vault carries only the notes and images a clip made (NET-016's rule). The import of clips
    is one task of the busy registry ("Importing saved web pages…", not cancellable, JOB-003),
    runs on a worker, writes at once with no Import dialog (the clip is the person's gesture in the
    browser) and reuses the Import code for names, media folder, atomic writes and snapshot. A
    clip whose target vault is gone stays in the spool and waits; only a hostile or broken file is
    quarantined. The user guide documents it (CLIP-069).
12. **Known limits** are written down, not hidden: cross-origin iframes, canvas, closed shadow
    roots, Flatpak browsers (other Snap browsers are not tested), and a custom PIN pad that shows digits as plain text in a
    `<span>`, which is saved like any visible text (CLIP-070).

## Alternatives considered

- **An embedded webview (wry/WebKitGTK/WebView2) in Shale.** Heavy, differs per platform, does not
  have the user's sign-ins, and the owner ruled embedding out.
- **Servo.** Not mature enough to render arbitrary real-world pages reliably, and a large
  dependency.
- **CEF or Electron.** Ruled out by the owner: size, update burden and attack surface.
- **A headless browser started by Shale for this use case.** It needs a browser installed anyway,
  starts without the user's sign-ins and session state (so paywalled or logged-in pages fail), and
  shows a different page than the one the user is looking at.
- **A localhost HTTP or WebSocket server in Shale.** A listening port that any local process or,
  through the browser, any web page can try to reach (DNS rebinding, cross-origin requests); needs
  pairing, tokens and a running Shale. Native messaging is started by the browser for one ID only.
- **A KeePassXC-style socket with encryption.** KeePassXC encrypts because it hands out
  passwords over a long-lived socket. Here data flows one way, into a folder; encryption and
  pairing add code and failure modes without protecting anything the spool permissions do not.
- **A downloads-API transport (the extension downloads a file, Shale watches the Downloads
  folder).** Lands in a folder other programs write to, has no caller check or reply, and the user
  sees download noise.
- **MHTML or `pageCapture`.** Saves the whole page with scripts, forms and form values; it is the
  opposite of an allowlist and needs a heavy parser in Shale. Chromium only.
- **A denylist sanitiser** (remove password fields, fields named pin/otp, `on*` attributes,
  scripts and so on, work on a clone). Rejected in the debate: anything the list forgets is sent
  (a field named differently, a typed value reflected into an attribute, a custom element), and
  the PIN-name regex and a "skip the whole form" rule are guesses. An allowlist emits only what is
  known to be safe and cannot leak a form control it never emits.
- **Shale downloading image URLs.** SSRF into the LAN and homelab, DNS rebinding, `file://` and
  other schemes, redirects, names that make paths, hostile files (polyglots, SVG with script or
  XXE, decompression bombs), resource exhaustion and tracking by the page. The browser already has
  the protections and the user's sessions, so it fetches; Shale only decodes and re-encodes.
- **Saving video.** MSE, HLS and DASH streams have no file (`blob:` MediaSource); DRM must not be
  circumvented; a plain `mp4` is rare and huge; cleaning video needs ffmpeg with a large attack
  surface; copyright. A link and a poster keep the useful part.

## Consequences

- A second build artefact (the extension: JS, HTML, manifest) lives in the repository and needs its
  own release path and store listings (Firefox add-ons, Chrome Web Store); a fixed extension ID has
  to be kept stable.
- The Shale binary gets a small, GUI-free native-host mode, and a spool importer that runs when
  Shale starts and while it runs.
- New dependencies are limited to what decoding needs (the `image` crate with limits); no HTTP
  library, no server.
- Tests need a browser: Playwright with the extension loaded, in a podman stack; some checks stay
  by hand (Firefox menu greying, real profiles). Chrome and Firefox differ in menus (hidden vs.
  greyed selection entry, CLIP-021, CLIP-022).
- Users of Flatpak browsers cannot use the clipper until that is solved (Mozilla bug 1955255); Snap Firefox needs the portal preference, other Snap browsers are not tested (CLIP-015).
- A text-only clip is small and safe; images and the permission for them are an explicit choice
  the user makes on the first-run page.
- Known leaks remain by design and are documented (CLIP-070).
