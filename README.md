# Shale

[![build](https://github.com/rabalyn/shale/actions/workflows/build.yml/badge.svg)](https://github.com/rabalyn/shale/actions/workflows/build.yml)

> [!WARNING]
> **Shale is built entirely by AI coding agents.**
>
> All of Shale (its code, tests and documentation) is written by AI agents through agentic
> coding, not by hand. The owner decides what Shale should do, sets and approves the specs and
> design decisions in `docs/`, reviews the changes and tests the app, but does not write the code.
>
> Some people do not want to use, support or contribute to software made this way. If that is
> you, Shale is not for you, and that is a fair choice. Every release page carries the same notice.

A native Rust notes app for the desktop: Linux, macOS and Windows (MIT licensed). Your vault is a plain folder of
markdown files. Notes link to each other with `[[wiki links]]`.

- **Links you can see**: `[[Note]]` is **blue** when the note exists and **red** when it doesn't.
  Click a blue link to open the note. Click a red link to create the note and open it.
- **Suggestions while you type**: type `[[` to get matching note names. Enter picks one, and the
  last entry creates a new note.
- **Drawings**: hand-drawn style sketches stored as standard `.excalidraw` JSON files.
  Put `![[Sketch.excalidraw]]` in a note to show the drawing there.
- **To-do list for the whole vault**: every `- [ ] task` from every note in one list.
  Tick tasks off there (the change is saved to the note), filter them, and sort by due date
  (`📅 2026-12-01` or `due:2026-12-01`).
- **Feeds**: read your Mastodon and Bluesky timelines in Shale and save a post (or a whole
  thread by one author) as a note, with its pictures and a link back to the post.
- **Linked notes**: backlinks, unlinked mentions, outgoing links, outline, a local graph per note
  and a graph of the whole vault.
- **Mail**: read your IMAP mail next to your notes, search it, mark it read, flag, move or delete
  it, write, reply, forward and send mail (plain text, attachments, drafts, an outbox with "undo
  send"), and turn a mail into a note with one key. Shale keeps a local copy of the mail; syncing never
  changes anything on the server, only your own actions do (delete always means the Trash folder;
  a sent mail is never sent twice, and a mail that may or may not have gone out is shown to you
  instead of being guessed). **The local mail copy is unencrypted (like Thunderbird's): use full-disk encryption, and encrypted swap too: Shale locks typed passwords and tokens into memory and wipes them when done, but where the system refuses the lock they could reach a swap file.**
  **Signed and encrypted mail** (OpenPGP and S/MIME) works through the GnuPG on your computer
  (`gpg`, `gpgsm`, your keys, your passphrase dialog): signatures are checked and shown in a strip above
  the mail, you can sign and encrypt what you send, and nothing is fetched from key servers by itself.
  **Read this first: with encryption switched on (Accounts…, Encryption; you must type a sentence to
  confirm), encrypted mails WILL ALWAYS BE STORED DECRYPTED LOCALLY** (the decrypted copy and its search
  text lie unencrypted in the mail folder, next to the encrypted original). Without that switch nothing
  is decrypted. To use your Thunderbird keys, export them there and import them into GnuPG
  (`gpg --import backup.asc`; S/MIME: `gpgsm --import backup.p12`), see below.
  Sign in with a password or with OAuth2 (Google, Microsoft). Shale has no OAuth2 client IDs of
  its own yet, so you register one (about five minutes, free):
  - *Google:* Cloud console, new project, OAuth consent screen (External, Testing, add yourself as
    a test user), then Credentials, OAuth client ID, type **Desktop app**.
  - Enter the client ID and the client secret in Shale (Accounts…, OAuth2, Google), then
    "Sign in in the browser".
  - Google's scope `https://mail.google.com/` is restricted: an unverified app works for up to 100
    test users, and while it is in Testing, Google ends the sign-in after 7 days.
  - *Microsoft:* Entra admin center, App registrations, new registration, platform "Mobile and
    desktop applications" with redirect address `http://localhost`, allow public client flows,
    permissions `IMAP.AccessAsUser.All` and `SMTP.Send` (for sending).
  - Enter the Application (client) ID in Shale (Accounts…, OAuth2, Microsoft); no secret needed.
  Details in [docs/FEATURES.md](docs/FEATURES.md). A local test mail server with an OAuth2
  provider (podman) is in [tests/mail-env](tests/mail-env/README.md).
  A local CalDAV server (Radicale, podman) with seeded calendars is in [tests/caldav-env](tests/caldav-env/README.md).
  - *Your keys from Thunderbird:* Thunderbird keeps OpenPGP keys in its own store, not in GnuPG's. In
    Thunderbird: Account settings, End-to-End Encryption, your key, "Backup secret key to file" (the
    public keys of others: the key manager, Export). Then `gpg --import file.asc`. S/MIME certificates:
    Settings, Privacy & Security, Manage S/MIME Certificates, Backup; then `gpgsm --import file.p12`
    and put the fingerprint of the issuer (for a self-signed one: of the certificate) into
    `~/.gnupg/trustlist.txt`. In Shale: Accounts…, the account, "Signing and encrypting": pick the key.
    Keys of your correspondents: `gpg --locate-keys name@example.org` (Shale never looks them up itself).

See **[docs/FEATURES.md](docs/FEATURES.md)** for all features with screenshots.

## Supported systems

| System | Tested | Notes |
|---|---|---|
| Linux (Wayland and X11), x86_64 | on every push (Forgejo) and for every release | the release download is for this system |
| macOS (Apple silicon, arm64) | built, linted and tested on GitHub (`macos-latest`) for every push to `main` | build it from source for now |
| Windows 10 and 11 (x86_64) | built, linted and tested on GitHub (`windows-latest`) for every push to `main` | build it from source for now |

Release downloads for macOS and Windows do not exist yet. What differs from Linux:

- **Passwords** and tokens go to the macOS Keychain and the Windows Credential Manager (on Linux to
  the Secret Service through `secret-tool`). If the store cannot be used, or you cancel its unlock
  prompt, Shale says so: they are then kept in the settings file, which only you can read, or for this run only.
- **Signed and encrypted mail** needs GnuPG on your computer (`brew install gnupg` on macOS,
  [Gpg4win](https://www.gpg4win.org) on Windows); without it Shale says so and everything else works.
- **CalDAV** and **git sync** need `curl` and `git` on the path (both come with macOS and Windows 10 and later; Git
  for Windows is a separate install).
- **Drag and drop**: files dropped on the window are imported everywhere; mail messages, links and
  text dragged out of Thunderbird need Wayland.
- **Time zones**: calendar events in a named zone (`Europe/Berlin`) use the system's zoneinfo
  files; Windows has none, so Shale brings its own time zone database there.

## Download

Released versions are on the [Releases page](https://github.com/rabalyn/shale/releases), each
with its notes from [CHANGELOG.md](CHANGELOG.md): download `shale-<version>-linux-x86_64.tar.gz`,
unpack it and run `./shale/shale`. Releases are built by GitHub Actions
(`.github/workflows/build.yml`); there are no builds between releases. To try an unreleased
version, build it from source (below).

Each download has a `.sha256` file next to it; to check the download, put both files in one folder and run
`sha256sum -c shale-<version>-linux-x86_64.tar.gz.sha256`.
This catches a damaged download, not a compromised release page.

To check that a download was built by Shale's release workflow from its version tag, use the
[GitHub CLI](https://cli.github.com/) (each release is attested through Sigstore, without a key):

```sh
gh attestation verify shale-<version>-linux-x86_64.tar.gz --repo rabalyn/shale \
  --signer-workflow rabalyn/shale/.github/workflows/build.yml --source-ref refs/tags/v<version>
```

Add `--bundle shale-<version>-linux-x86_64.tar.gz.sigstore.json` to use the attestation file from
the release page instead of asking GitHub for it. This shows where and from what the file was
built; it does not show that a person checked the build.

### Making a release

1. Raise `version` in `Cargo.toml` ([semver](https://semver.org/): before 1.0, new features raise
   the middle number, fixes the last one) and run `cargo build` (updates `Cargo.lock`).
2. Move the notes from `## [Unreleased]` in `CHANGELOG.md` to a new `## [x.y.z] - date` section.
3. Commit, then run `sh make-release.sh`: it tags the commit `vx.y.z` and pushes the commit and
   the tag to the development server (remote `origin`) and to GitHub (remote `github`). GitHub
   builds it and creates the release page with the AI notice (`.github/ai-notice.md`) and that
   section as its notes and the `.sha256` file.
   Before tagging, and again in both CI systems, `.github/check-release.sh` checks that the
   version is valid semver and greater than the previous `v*` tag, that `Cargo.toml` and
   `Cargo.lock` agree, and that the changelog section is not empty and mentions every issue
   closed (`Closes #N`) since the previous tag; otherwise nothing is tagged or built.

Day-to-day development is on a self-hosted Forgejo, whose CI (`.forgejo/workflows/ci.yml`)
formats, lints, builds, tests, packages and audits every pull request and `main`; GitHub gets the
released versions, and builds and tests macOS and Windows for every push to `main` (the Linux
job of GitHub only runs for release tags).

## Development and issues

Shale is developed on Forgejo: <https://git.home.rabalyn.de/raba/shale>. Branches, pull requests
and the **issues, which are the one list of open work**, live there; GitHub only builds and hosts the
releases ([ADR-0013](docs/decisions/0013-forgejo-and-issue-tracking.md), FORGE rules in
[engineering.md](docs/spec/engineering.md)). Checks run on the Forgejo runner
(`.forgejo/workflows/ci.yml`); GitHub builds release tags (BUILD-012) and tests macOS and
Windows (BUILD-017); a red run there is reported back as a Forgejo issue (BUILD-020).

## Build and run

```sh
cargo build --release
./target/release/shale                    # opens the last vault, or the start screen
./target/release/shale --vault ~/Notes    # opens a folder as vault
```

You need a Rust toolchain (1.89 or newer; CI uses the version in `rust-toolchain.toml`). On Linux also
the usual desktop libraries (Wayland or X11, plus OpenGL/Vulkan drivers, and a C compiler for the
bundled SQLite). On macOS the Xcode command line tools (`xcode-select --install`) are enough. On Windows
install Rust with the MSVC toolchain (the Visual Studio Build Tools with "Desktop development with C++");
the same three commands work in PowerShell, with `.\target\release\shale.exe` as the program.

Other options:

- `--screenshot out.png` starts with a temporary demo vault, draws about 120 frames, saves a
  picture of the window and exits. Automated checks use it.
- `--view tasks|graph|drawing|reading|calendar|mail|social|social-import|keybindings|data-usage` (with `--screenshot`) chooses which
  view to capture; `mail` uses a made-up mail store (no server, no real mail), `social` and
  `social-import` made-up posts; `open:<file>` opens a file of the vault, `read-only:<file>.canvas`
  shows a canvas without the question, and `convert:<file>.canvas` converts it first.
- `--size 1360x840` (with `--screenshot`) sets the window size, and `--keys 'step|step|…'` scripts typing,
  keys and the mouse (`click:@Sync`, `rclick:347,21`, `drag:@Ideas>@Projects`, `waitfor:@Reply`; the
  steps are listed at the top of `src/script.rs`).
- An option Shale does not know, or one without its value, stops the start with an error and the
  usage line (exit code 2) instead of being skipped.
- The pictures of `docs/screenshots/` are made by `docs/screenshots/regen.sh` (all, or the names
  given) from `docs/screenshots/recipes.conf`; it runs each picture in a private X display
  (`xvfb-run`) and needs a built `shale` that is not older than the sources (`SHALE=…` to choose it).

## Keyboard

The keys below are the defaults of the shortcuts that can be changed in **Settings → Keybindings** (Change replaces the keys of an action, Add gives it one more, Clear leaves it without a key)
(everywhere, file list, search results, mail view, vim normal mode; double bindings are refused).
Text editing keys and vim keys are fixed.

| Keys | Action |
|---|---|
| Ctrl+N | New note |
| Ctrl+O | Quick switcher (find or create a note) |
| Ctrl+E | Switch between editing and reading view |
| Ctrl+Shift+F | Search all notes |
| Ctrl+H / Ctrl+L | Keyboard into the left sidebar / back into the editor |
| Ctrl+Alt+H / Ctrl+Alt+L | Previous / next tab (from the last tab on to the first), wherever the keyboard is |
| Ctrl+Shift+M | Mail view (its keys are listed below) |
| Ctrl+Shift+B | File list with the keyboard in it: j / k (or ↓ / ↑) next / previous file, Ctrl+J / Ctrl+K next / previous folder, Space opens or closes a folder, Enter opens, Esc back |
| ↓ or Enter in the search box | Browse the results: j / k (or ↓ / ↑) next / previous hit, Ctrl+J / Ctrl+K next / previous file, Enter opens the note at the hit, Esc back to the box |
| Ctrl+S | Save now (notes also save automatically) |
| Ctrl+W | Close tab |
| Drag a tab | Reorder tabs; a marker shows where it lands |
| Right-click a tab | Pin tab / Unpin tab (pinned tabs sit at the left with a pin and keep their place between starts) |
| Alt+← / Alt+→ | Back / forward |
| Ctrl+6 | Alternate view, in every view (also mail and drawings, where Space is taken): swap with the view this tab showed just before; again: back |
| Space Tab | Vim-style editing, normal mode: back to the view this tab showed just before (note, calendar, Feeds, …); again: forward to this one |
| Space … | A started key sequence (e.g. `Space f`) shows in the status bar and is dropped after 1.5 s without a key |
| Space f f | Vim-style editing, normal mode: find a file by name (↓ / ↑ or Ctrl+J / Ctrl+K select, Enter opens it in this tab, Esc closes) |
| Space f g | Vim-style editing, normal mode: search the text of notes, mail and posts (`*` and `?` are wildcards; results grouped with counts; ↓ / ↑ or Ctrl+J / Ctrl+K select, Enter opens the hit where it belongs, Esc closes) |
| Ctrl+, | Settings |
| Ctrl+Shift+D | Data usage: what Shale keeps on disk, and its caches |
| Click on a link | Open it (or create it if it's red) |
| Ctrl+click on a link | Put the cursor in it so you can edit it (you can swap this in Settings) |
| Middle-click on a link | Open it in a new tab |

### Mail view keys

In the mail view (Ctrl+Shift+M) one of the three panes (folders, message list, message) has the
keyboard; it has an accent-coloured frame. **Ctrl+H / Ctrl+L** move it one pane left / right (from
the folder pane Ctrl+H goes on into the left sidebar, from there Ctrl+L comes back). In a window
too narrow for the three panes they scroll sideways, and the pane with the keyboard is brought into
view. Letters work only while no text field has the keyboard. Every key can be changed in Settings → Keybindings
(scope Mail view); Enter and Esc are fixed.

| Keys | Action |
|---|---|
| j / ↓, k / ↑ | Next / previous: folder in the folder pane, message in the list, scroll in the message pane |
| Enter | Folder pane: go to the list; list: open the message (Drafts: open the draft; Outbox: show the mail) |
| Ctrl+J / Ctrl+K | Next / previous folder, from any pane |
| Space | Mark the message for a selection (list) |
| / | Search mail (Esc leaves the field, ↓ or Enter go into the results) |
| t | Search all folders or only this one |
| i | Import to vault |
| u | Mark read or unread |
| s or f | Set or clear the flag |
| d or Delete | Delete (Trash: asks first); in Drafts and Outbox discard the draft or the record |
| m | Move to a folder |
| c | Write a new mail |
| r, Shift+R, Shift+F | Reply, reply all, forward |
| Ctrl+R | Sync, or cancel the sync |
| a | Account settings |
| Shift+L | Load the open message (or the list of a folder that is not synced) from the server |
| ] | Select the next attachment (marked with ▸) |
| o, Shift+O | Open the selected attachment, save it to the vault |
| z | Take back a mail waiting in the outbox |
| e | Edit: open the draft, or take a mail of the outbox that was not sent back into the editor |
| Shift+S | Retry a mail of the outbox that was not sent (or send it again when it may have been sent) |
| Menu, Shift+F10 | Open the menu of the message or draft under the cursor (↓ / ↑ or j / k, Enter, Esc) |
| Shift+U | Show or hide the sent mails that are only on this computer (j / k, Enter = Upload again, d = Dismiss, Esc) |
| Esc | Step back: close the message, clear the search, clear the marks, dismiss a message |

With **Vim-style editing** turned on in Settings, a note you go to is in normal mode:
h j k l, w b, W B, 0 $, gg G move; `:12` goes to line 12; `/text` searches, n / N go to the next /
previous hit; i a I A o O start typing and Esc stops; x deletes a character; v / V select (the
motions extend the selection), then d cuts and y copies to the clipboard; p pastes it. The
status bar shows the mode. **Space Tab** goes back to the view this tab showed just before
(a note, the calendar, Feeds, …) and again to come back; **Space f f** finds a file by name and **Space f g** searches notes, mail and posts.

In a drawing: V/1 select, H hand, R/2 rectangle, D/3 diamond, O/4 ellipse, A/5 arrow, L/6 line,
P/7 pen, T/8 text, E/0 eraser. Ctrl+Z/Ctrl+Shift+Z undo/redo, Ctrl+D duplicate, Delete removes.
The mouse wheel pans and Ctrl+wheel zooms. Hold Space and drag to pan.

## Tests

```sh
cargo test
```

The tests also run against a real vault if there is one: put notes, drawings and files into
`notes/` next to `Cargo.toml` (it is not committed), or point `SHALE_TEST_VAULT` at a folder.
Each test works on a copy, so the vault itself is never changed. Without such a folder those
tests pass with a note. On macOS and Linux the test of `scripts/report-github-ci.sh` (BUILD-020) needs
`sh`, `curl` and `jq` on the path and fails with a message that says so when one is missing.

CI also measures line coverage (`cargo llvm-cov`; gaps and the ratchet in
[docs/coverage.md](docs/coverage.md)) and runs the real-server CalDAV and mail suites in podman
(`cargo test -- --ignored calendar::env_tests` / `mail_env`, see [tests/caldav-env](tests/caldav-env/README.md)).

The macOS and Windows code can be checked on Linux before pushing (DEV-015):

```sh
sh scripts/cross-check.sh          # clippy for Windows and macOS
sh scripts/cross-check.sh --wine   # also the Windows tests under Wine (about 5 minutes)
```

It needs the Rust targets `x86_64-pc-windows-gnu` and `aarch64-apple-darwin`, `mingw-w64-gcc`
for Windows, `zig` for macOS and `wine` for `--wine`, and it says which one is missing. Tests
that fail only under Wine are listed with their reason in `scripts/wine-known-failures.txt`
and skipped. GitHub's macOS and Windows jobs stay the real check: they build with the
platform's own toolchain, run real Windows and run the macOS tests.

See [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md) for what is done, what is tested and what is
still missing. What Shale guarantees is in the specs ([docs/spec/](docs/spec/README.md)), and
why it is built that way is in the decision records ([docs/decisions/](docs/decisions/README.md)).
