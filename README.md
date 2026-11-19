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
- **A formatting toolbar and colours**: a row of buttons above the text for bold, headings, lists, links,
  tables and more, and coloured highlights and font colours stored as plain `<mark>` / `<span>` HTML.
- **Math**: `$E = mc^2$` and `$$…$$` are typeset in the text, the reading view, tables and the PDF export,
  by Shale itself with the bundled font STIX Two Math (no TeX needed).
- **Themes**: nine built-in light and dark colour themes (Shale, Dracula, Nord, Catppuccin, Gruvbox, Solarized,
  GitHub), one that follows the system's light or dark mode, and any Alacritty terminal theme file you put into
  the themes folder.
- **Suggestions while you type**: type `[[` to get matching note names. Enter or a click picks one, and the
  last entry creates a new note.
- **Drawings**: hand-drawn style sketches stored as standard `.excalidraw` JSON files.
  Put `![[Sketch.excalidraw]]` in a note to show the drawing there.
- **To-do list for the whole vault**: every `- [ ] task` from every note in one list.
  Tick tasks off there (the change is saved to the note), filter them, and sort by due date
  (`📅 2026-12-01` or `due:2026-12-01`).
- **Feeds**: read your Mastodon and Bluesky timelines in Shale and save a post (or a whole
  thread by one author) as a note, with its pictures and a link back to the post.
- **Linked notes**: backlinks, unlinked mentions, outgoing links, outline, a local graph per note
  and a graph of the whole vault. Headings can be numbered (1, 1.1, 1.2), a note's title line hides a `YYMMDD_` date prefix, links to a heading
  scroll to it, and the PDF export has a table of contents and bookmarks, and a note's `print-*` properties set its paper, margins, font, spacing and more; a folder, or the whole vault, exports as one PDF, a book, with a title page, contents, running headers and links between the chapters (`shale --export-pdf <folder> <file.pdf>` does it without a window). A note, or a folder as a thesis or report, can also be
  exported through LaTeX templates (the TU Darmstadt classes, for instance): a guided setup finds or
  installs the TeX, imports the templates and asks for your logo and details. The built-in export needs
  nothing; the LaTeX export needs a TeX, which Shale installs for you only when you agree (about 100 MB to
  download, 330 MB on disk) unless your computer has one, and the university's logo is not part of the templates
  and must be supplied by you.
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
  To stay unlocked longer, Accounts…, Encryption can lengthen GnuPG's passphrase cache with `gpgconf`
  (shown first, written only on a second click) or shows the lines to put in `gpg-agent.conf` yourself.
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

**User guide**: `docs/userguide` is a Shale vault: it shows the features it explains. In Shale press **F1**
(or Settings → Help → **Open user guide**): the guide is built into the program, which unpacks it once per
version into its data folder (`~/.local/share/shale/userguide-<version>/` on Linux; a copy you edited is never
overwritten) and opens it as a vault, at "Start here". This works in every build, on every system. Each release also has `shale-userguide-<version>.zip` (with
its `.sha256` file) to unpack anywhere and open as a vault (`shale --vault <folder>`, or Settings → Vault folder),
and `shale-userguide-<version>.pdf`, the same guide as one book to print or read anywhere (made by
`scripts/userguide-pdf.sh`).

## Supported systems

| System | Tested | Notes |
|---|---|---|
| Linux (Wayland and X11), x86_64 | before every change lands, for every check tag (Forgejo) and for every release | release download: `shale-<version>-linux-x86_64.tar.gz` |
| macOS (Apple silicon, arm64) | built, linted and tested on GitHub (`macos-latest`) for every release; clippy before every change lands | release download: `shale-<version>-macos-arm64.dmg`, not notarised (see Download) |
| Windows 10 and 11 (x86_64) | built, linted and tested on GitHub (`windows-latest`) for every release; clippy before every change lands | release download: `shale-<version>-windows-x86_64.zip`, not signed (see Download) |

What differs from Linux:

- **Passwords** and tokens go to the macOS Keychain and the Windows Credential Manager (on Linux to
  the Secret Service through `secret-tool`). If the store cannot be used, or you cancel its unlock
  prompt, Shale says so: they are then kept in the settings file, which only you can read, or for this run only.
- **Signed and encrypted mail** needs GnuPG on your computer (`brew install gnupg` on macOS,
  [Gpg4win](https://www.gpg4win.org) on Windows); without it Shale says so and everything else works.
- **Programs from Homebrew** (macOS): an app started from Finder or the Dock gets only the system's folders on its `PATH`, so Shale adds `/opt/homebrew/bin`, `/opt/homebrew/sbin` and `/usr/local/bin` after them at start; `git-lfs`, `ffmpeg`, `gpg` and `pandoc` from Homebrew are found without starting Shale from a terminal.
- **Videos** in notes are played with `ffmpeg` and `ffprobe` on your computer (`brew install ffmpeg` on macOS,
  `winget install ffmpeg` on Windows, your package manager on Linux). Without them a video opens in your
  system's video player and Settings says so; everything else works.
- **Word files** (`.docx`) are imported as notes by `pandoc` on your computer, once you turn it on in Settings →
  Import (`brew install pandoc` on macOS, `winget install JohnMacFarlane.Pandoc` on Windows, your package
  manager on Linux). Without it a `.docx` is saved as an attachment and the Import dialog says so.
- **CalDAV** and **git sync** need `curl` and `git` on the path (both come with macOS and Windows 10 and later; Git
  for Windows is a separate install).
- **Media files in git sync** (audio and video, stored with Git LFS) need `git-lfs` as well
  (<https://git-lfs.com>; `apt install git-lfs`, `brew install git-lfs`; Git for Windows brings it). Without it Shale
  says so in the settings and everything else works; a vault that already stores media with LFS is not synced on a
  computer without git-lfs, so no media file lands in the history as a plain file. On another computer a media
  file arrives as a small pointer and is downloaded when you play or open it (or after every sync, if you switch
  that on in the git sync settings).
- **Drag and drop**: files dropped on the window are imported everywhere (pictures, PDFs, audio and
  video as files in the media folder, pasting a picture into the editor does the same); mail messages, links and
  text dragged out of Thunderbird need Wayland.
- **Time zones**: calendar events in a named zone (`Europe/Berlin`) use the system's zoneinfo
  files; Windows has none, so Shale brings its own time zone database there.

## Download

Released versions are on the [Releases page](https://github.com/rabalyn/shale/releases), each
with its notes from [CHANGELOG.md](CHANGELOG.md): download `shale-<version>-linux-x86_64.tar.gz`
(Linux), `shale-<version>-macos-arm64.dmg` (macOS, Apple silicon) or `shale-<version>-windows-x86_64.zip` (Windows),
unpack it and run `./shale/shale` (`shale\shale.exe` on Windows); on macOS open the dmg and drag `Shale` onto the `Applications` link. (The user guide is built into the program, is in the Linux and Windows archives as `shale/docs/userguide/`, and
alone as `shale-userguide-<version>.zip` and as a PDF, `shale-userguide-<version>.pdf`.) On Linux, `shale/install.sh` installs the program, the app menu entry and the icons (on Wayland the
taskbar needs the entry to show the icon), on any distribution:

```sh
cd shale
./install.sh                     # for you: ~/.local/bin and ~/.local/share
sudo ./install.sh --prefix /usr/local   # for all users: /usr/local/bin and /usr/local/share
./install.sh --uninstall         # removes what it installed (add the same --prefix); your vaults stay
```

On Arch Linux the same download is packaged as `shale-bin` (`packaging/aur/shale-bin/` has the PKGBUILD, BUILD-026): build it from that folder with `makepkg -si`, or once the owner has published it on the AUR, `yay -S shale-bin` or `paru -S shale-bin`.

It works from any folder, runs again to upgrade, warns when the program's folder is not on your `PATH`
and names libraries your system lacks (with the package names for Arch, Debian and Fedora). There is
no AppImage and no Flatpak: an AppImage needs FUSE 2 and brings no menu entry, and a Flatpak sandbox
would block your vault folders and the programs Shale starts (git, gpg, ffmpeg, curl, pandoc).

The macOS and Windows downloads are not notarised or signed with a developer certificate, so the system warns before the first start:

- **macOS** (Gatekeeper): `Shale.app` is signed ad hoc only, so macOS says it is from an unidentified developer. Right-click
  `Shale` in Applications and choose Open (once), or choose Open Anyway in System Settings, Privacy & Security after the first
  try, or run `xattr -dr com.apple.quarantine /Applications/Shale.app`. The ad-hoc signature changes with every build, so the
  Keychain may ask again for access to your saved passwords after each update (choose Always Allow).
- **Windows** (SmartScreen) shows "Windows protected your PC": choose More info, then Run anyway.

Releases are built by GitHub Actions
(`.github/workflows/build.yml`); there are no builds between releases. To try an unreleased
version, build it from source (below).

Each download has a `.sha256` file next to it; to check the download, put both files in one folder and run
`sha256sum -c shale-<version>-linux-x86_64.tar.gz.sha256` (the same for `shale-userguide-<version>.zip` and `shale-userguide-<version>.pdf`).
On macOS use `shasum -a 256 -c shale-<version>-macos-arm64.dmg.sha256`; on Windows run
`certutil -hashfile shale-<version>-windows-x86_64.zip SHA256` (or PowerShell `Get-FileHash`) and compare the hash with the
one in the `.sha256` file.
This catches a damaged download, not a compromised release page.

To check that a download was built by Shale's release workflow from its version tag, use the
[GitHub CLI](https://cli.github.com/) (each release is attested through Sigstore, without a key):

```sh
gh attestation verify shale-<version>-linux-x86_64.tar.gz --repo rabalyn/shale \
  --signer-workflow rabalyn/shale/.github/workflows/build.yml --source-ref refs/tags/v<version>
```

Add `--bundle shale-<version>-linux-x86_64.tar.gz.sigstore.json` to use the attestation file from
the release page instead of asking GitHub for it (one file attests the downloads of all three systems; use the same
command with the macOS or Windows file name). This shows where and from what the file was
built; it does not show that a person checked the build.

### Making a release

Tags are made by two scripts ([ADR-0018](docs/decisions/0018-tag-only-ci.md)); both bump the
version in `Cargo.toml` and `Cargo.lock` ([semver](https://semver.org/): before 1.0, new features
raise the middle number, fixes the last one), land the bump with `scripts/land.sh`, tag the landed
commit and push the tag to the development server (remote `origin`), whose CI runs only for tags.
`--dry-run` shows the tag without changing anything. Without a changelog update (for a check tag:
nothing new in `CHANGELOG.md` or `docs/changelog-pending/` since the previous tag; for a release:
no entries at all) both stop with "the changelog needs an update first".

1. **Check tag**, when the work is done for now: `scripts/make-tag.sh minor` (or `patch`,
   `major`) makes `v0.4.0-rc.1`, and `scripts/make-tag.sh` the next `rc`. It merges the files in
   `docs/changelog-pending/` into `## [Unreleased]`. When the development server's CI passes,
   `main` is pushed to GitHub, which builds nothing; the tag stays there.
2. **Release**: `scripts/make-release.sh` releases the version the check tags led to (`0.4.0`).
   It also turns `## [Unreleased]` in `CHANGELOG.md` and the files in `docs/changelog-pending/`
   into the section `## [0.4.0] - date`. When the development server's CI passes, `main` and the
   tag go to GitHub, which builds it and creates the release page with the AI notice
   (`.github/ai-notice.md`) and that section as its notes and the `.sha256` file.
   Before tagging, and again in both CI systems, `.github/check-release.sh` checks that the
   version is valid semver and greater than every other `v*` tag, that `Cargo.toml` and
   `Cargo.lock` agree, and, for a release, that the changelog section is not empty and mentions
   every issue closed (`Closes #N`) since the previous release; otherwise nothing is tagged or
   built.

Day-to-day development is on a self-hosted Forgejo, whose CI (`.forgejo/workflows/ci.yml`)
formats, lints, builds, tests and packages every tag and audits the dependencies daily
(`security.yml`); GitHub gets `main` with each green tag and builds only the releases, on Linux,
macOS and Windows (`.github/workflows/build.yml` runs for release tags and by hand, never for a
push or a check tag).

## Development and issues

**Issues are here on GitHub** (<https://github.com/rabalyn/shale/issues>), and they are the one
list of open work: report a problem or ask for a feature there. Report a vulnerability privately
instead ([SECURITY.md](SECURITY.md)). Every advisory on a dependency that `cargo audit` finds becomes
an issue labelled `security` by itself (BUILD-021). Development and day-to-day CI happen on the
owner's self-hosted Forgejo, which is not public; `main` is pushed here after its checks passed
([ADR-0017](docs/decisions/0017-github-issues-forgejo-ci.md), FORGE rules in
[engineering.md](docs/spec/engineering.md)). The issue numbers before 2026-11-17 come from Forgejo
and were imported with their numbers, so `#N` in the history is the issue `#N` here. Changes land locally: `scripts/land.sh <branch>`
merges a branch into `main`, runs fmt, clippy and the tests, and pushes only when they pass
([ADR-0016](docs/decisions/0016-local-landing.md)). The Forgejo runner checks again for every tag
(`.forgejo/workflows/ci.yml`, [ADR-0018](docs/decisions/0018-tag-only-ci.md)); GitHub builds
release tags (BUILD-012), also on macOS and Windows (BUILD-017). How landings, builds, agents,
issues and releases are doing is exported for Prometheus and Grafana ([docs/dev-metrics.md](docs/dev-metrics.md)).

## Build and run

```sh
cargo build --release
./target/release/shale                    # opens the last vault, or the start screen
./target/release/shale --vault ~/Notes    # opens a folder as vault
```

You need a Rust toolchain (1.89 or newer; CI uses the version in `rust-toolchain.toml`). On Linux also
the usual desktop libraries (Wayland or X11, plus OpenGL/Vulkan drivers, and a C compiler for the
bundled SQLite) and the ALSA headers for sound (`libasound2-dev` on Debian and Ubuntu, `alsa-lib` on
Arch, `alsa-lib-devel` on Fedora). On macOS the Xcode command line tools (`xcode-select --install`) are enough. On Windows
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
- Performance: a build with `--features bench` has `--bench`, which runs headless benchmarks of the
  real code paths (and a worst case with everything loaded at once) and writes JSON; see
  `docs/perf/README.md` for `scripts/bench.sh` (with flamegraphs), `scripts/bench-compare.sh` and the
  report `docs/perf/index.html`. Releases store their results in `docs/perf/results/`.
- An option Shale does not know, or one without its value, stops the start with an error and the
  usage line (exit code 2) instead of being skipped.
- The pictures of the user guide (`docs/userguide/z_media/screenshots/`) are made by
  `docs/screenshots/regen.sh` (all, or the names given) from `docs/screenshots/recipes.conf`; it runs each picture in a private X display
  (`xvfb-run`) and needs a built `shale` that is not older than the sources (`SHALE=…` to choose it).
- `shale --record clip.webm` runs a `--keys` script like `--screenshot` does, but records every frame
  as a short VP9 video (30 frames per second of app time, so the clip does not depend on the speed of the
  machine); it needs `ffmpeg` and says so when it is missing. The clips of the user guide
  (`docs/userguide/z_media/clips/`) are made by `docs/screenshots/regen-clips.sh` (all, or the names given)
  from `docs/screenshots/clips.conf`.

## Keyboard

Ctrl is Cmd on macOS. No default key is one that the system reserves (the app menu's Cmd+H, AltGr on Windows, the workspace keys of the Linux desktops), so macOS has other keys for six actions, named in the rows below; a key you set yourself is never refused for that reason. The keys below are the defaults of the shortcuts that can be changed in **Settings → Keybindings** (Change replaces the keys of an action, Add gives it one more, Clear leaves it without a key)
(everywhere, file list, search results, mail view, note view, vim normal mode; a key that another action uses asks whether to overwrite it: Yes takes it from that action, No changes nothing).
Text editing keys and vim keys are fixed.

| Keys | Action |
|---|---|
| Ctrl+N | New note |
| Ctrl+O | Quick switcher (find or create a note) |
| Ctrl+E | Switch between editing and reading view |
| Ctrl+Shift+F | Search all notes |
| Ctrl+H / Ctrl+L | Keyboard one place left / right: through the split panes in screen order, left of the first pane into the left sidebar, from there into the first pane (macOS: Cmd+Shift+H / Cmd+Shift+L, because Cmd+H hides the app) |
| Ctrl+PageUp / Ctrl+PageDown | Previous / next tab (from the last tab on to the first), wherever the keyboard is (macOS: Cmd+Opt+← / Cmd+Opt+→) |
| Ctrl+\ | Split right: open this tab in a new pane to the right (panes sit side by side, each with its own tabs) |
| F6 / Shift+F6 | Focus the next / previous pane |
| Ctrl+Shift+W | Close the pane with all its tabs (macOS: Cmd+Shift+W) |
| Ctrl+Shift+M | Mail view (its keys are listed below) |
| Ctrl+Shift+B | File list with the keyboard in it: j / k (or ↓ / ↑) next / previous file, Ctrl+J / Ctrl+K next / previous folder, Space opens or closes a folder, Enter opens, Esc back |
| ↓ or Enter in the search box | Browse the results: j / k (or ↓ / ↑) next / previous hit, Ctrl+J / Ctrl+K next / previous file, Enter opens the note at the hit, Esc back to the box |
| Ctrl+S | Save now (notes also save automatically) |
| Ctrl+W | Close tab |
| Drag a tab | Reorder tabs; a marker shows where it lands |
| Right-click a tab | Pin tab / Unpin tab (pinned tabs sit at the left with a pin and keep their place between starts); Open in split right |
| Alt+← / Alt+→ | Back / forward (macOS: Cmd+[ / Cmd+], because Option+← / → move by word in text) |
| Alt+B, Alt+I, Alt+S, Alt+H, Alt+C, Alt+Y, Alt+F, Alt+E, Alt+U, Alt+O, Alt+T, Alt+Q, Alt+K, Alt+R, Alt+J, Alt+A, Alt+L, Alt+W, Alt+M, Alt+X | The buttons of the formatting toolbar of a note, each doing what a click on it does (the keyboard stays in the text and nothing is typed): Bold, Italic, Strikethrough, Highlight, Inline code, Highlight colour and Font colour (open their palette, the ▾ part of the split button), Heading level (opens its menu), Bullet list, Numbered list, Task, Quote, Code block, Horizontal rule, Table, Link to a note, Web link, Embed, Clear formatting. A key whose button is not there (reading view) or is greyed out (in a table cell) does nothing |
| Ctrl+Shift+K / Ctrl+Shift+J | Fold / unfold the block at the cursor (a heading section, quote, list item or list; again on a folded line folds the block around it); the cursor then goes to the block's first line |
| Ctrl+Shift+O / Ctrl+Shift+I | Fold all / unfold all blocks of the note (also in the reading view) |
| Ctrl+Shift+C | Copy the code of the code block at the cursor (no fences, no newline at the end) |
| Alt+V, Alt+P, Alt+D, Alt+N | The buttons of the view header and tab bar of a note: edit / reading view, Markdown preview, the ⋯ menu (opens it), New tab |
| Alt+1, Alt+2, Alt+3, Alt+4, Alt+5, Alt+6, Alt+7, Alt+8, Alt+9, Alt+0 | The ribbon items by position: the first ten icons of the ribbon, as shown (also after you reordered or removed some) |
| Alt+, / Alt+. | Show or hide the left / right sidebar |
| Hold Alt | A small badge with its key appears at the corner of each of these buttons that has an Alt key (it follows a key you changed). On macOS, where Option+letter types characters, these actions, and the Alt keys of the open mail, have no key until you give them one in Settings → Keybindings (scope Note view, Mail view and Everywhere) |
| Ctrl+6 | Alternate view, in every view (also mail and drawings, where Space is taken): swap with the view this tab showed just before; again: back |
| Space Tab | Vim-style editing, normal mode: back to the view this tab showed just before (note, calendar, Feeds, …); again: forward to this one |
| Space … | A started key sequence (e.g. `Space f`) shows in the status bar and is dropped after 1.5 s without a key |
| Space f f | Vim-style editing, normal mode: find a file by name (↓ / ↑ or Ctrl+J / Ctrl+K select, Enter opens it in this tab, Esc closes) |
| Space f g | Vim-style editing, normal mode: search the text of notes, mail and posts (`*` and `?` are wildcards; results grouped with counts; ↓ / ↑ or Ctrl+J / Ctrl+K select, Enter opens the hit where it belongs, Esc closes) |
| Shift+K | Vim-style editing, normal mode: preview the link under the cursor (a note as in the reading view, a web page's title and text; press it again to move the keyboard into the preview: j / k or ↓ / ↑ scroll, Enter opens the link like `gt`, Ctrl+Enter opens it in a new pane on the right, Esc closes). Definable, e.g. as Space Space |
| Ctrl+, | Settings |
| Ctrl+Shift+D | Data usage: what Shale keeps on disk, and its caches |
| F1 | Open the user guide (built into Shale, unpacked into its data folder and opened as a vault) |
| Click on a link | Open it (or create it if it's red) |
| Ctrl+click on a link | Put the cursor in it so you can edit it (you can swap this in Settings) |
| Middle-click on a link | Open it in a new tab |

### Mail view keys

In the mail view (Ctrl+Shift+M) one of the three panes (folders, message list, message) has the
keyboard; it has an accent-coloured frame. **Ctrl+H / Ctrl+L** (macOS: Cmd+Shift+H / Cmd+Shift+L) move it one pane left / right (from
the folder pane Ctrl+H goes on into the pane on the left, or the left sidebar, from there Ctrl+L comes back). In a window
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
| Alt+I, Alt+C, Alt+R, Alt+A, Alt+F, Alt+U, Alt+G, Alt+M, Alt+D | The buttons of the open mail: Import to vault, Close, Reply, Reply all, Forward, Mark read / unread, Flag, Move…, Delete (the letter is underlined on the button while Alt is held; not on macOS, where Option+letter types characters: bind them in Settings → Keybindings) |
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
previous hit; i a I A o O start typing and Esc stops; x deletes a character; `dd` deletes the line;
v / V select (the motions extend the selection), then d cuts and y copies to the clipboard; p pastes it;
`u` undoes and Ctrl+R redoes (an insert session is one step, per note; the history is kept in the vault for 30 days); `gt` follows the link under the cursor like a click; **Shift+K** shows a preview of it (the note, or the title and text of a web page loaded in the background; Shift+K again moves the keyboard into it, Enter opens the link, Ctrl+Enter opens it in a new pane on the right, Esc closes it). The
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
tests pass with a note. On macOS and Linux the test of `scripts/report-advisories.sh` (BUILD-021) needs
`sh`, `curl` and `jq` on the path and fails with a message that says so when one is missing.

Debug setting of the TeX tests: `SHALE_TEX_FAILED_LOGS=<folder>` makes a failed LaTeX compile (Shale's own, and the
demos of `tex_env`) copy its full `.log` into that folder before its work folder is removed. `scripts/land.sh` sets it
(`target/tex-failed-logs`) and lists the kept logs (see `tests/tex-env/README.md`).

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
