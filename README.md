# Shale

[![build](https://github.com/rabalyn/shale/actions/workflows/build.yml/badge.svg)](https://github.com/rabalyn/shale/actions/workflows/build.yml)

A native Rust notes app for the Linux desktop (MIT licensed). Your vault is a plain folder of
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
  instead of being guessed). **The local mail copy is unencrypted (like Thunderbird's): use full-disk encryption.**
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
  - *Your keys from Thunderbird:* Thunderbird keeps OpenPGP keys in its own store, not in GnuPG's. In
    Thunderbird: Account settings, End-to-End Encryption, your key, "Backup secret key to file" (the
    public keys of others: the key manager, Export). Then `gpg --import file.asc`. S/MIME certificates:
    Settings, Privacy & Security, Manage S/MIME Certificates, Backup; then `gpgsm --import file.p12`
    and put the fingerprint of the issuer (for a self-signed one: of the certificate) into
    `~/.gnupg/trustlist.txt`. In Shale: Accounts…, the account, "Signing and encrypting": pick the key.
    Keys of your correspondents: `gpg --locate-keys name@example.org` (Shale never looks them up itself).

See **[docs/FEATURES.md](docs/FEATURES.md)** for all features with screenshots.

## Download

Released versions are on the [Releases page](https://github.com/rabalyn/shale/releases), each
with its notes from [CHANGELOG.md](CHANGELOG.md): download `shale-<version>-linux-x86_64.tar.gz`,
unpack it and run `./shale/shale`. Releases are built by GitHub Actions
(`.github/workflows/build.yml`); there are no builds between releases. To try an unreleased
version, build it from source (below).

Each download has a `.sha256` file next to it; to check the download, put both files in one folder and run
`sha256sum -c shale-<version>-linux-x86_64.tar.gz.sha256`.
This catches a damaged download, not a compromised release page.

### Making a release

1. Raise `version` in `Cargo.toml` ([semver](https://semver.org/): before 1.0, new features raise
   the middle number, fixes the last one) and run `cargo build` (updates `Cargo.lock`).
2. Move the notes from `## [Unreleased]` in `CHANGELOG.md` to a new `## [x.y.z] - date` section.
3. Commit, then run `sh make-release.sh`: it tags the commit `vx.y.z` and pushes the commit and
   the tag to the development server (remote `origin`) and to GitHub (remote `github`). GitHub
   builds it and creates the release page with that section as its notes and the `.sha256` file
   (the build stops if the tag and `Cargo.toml` disagree or the changelog has no notes for the
   version).

Day-to-day development is on a self-hosted Forgejo, whose CI (`.forgejo/workflows/ci.yml`)
formats, lints, builds, tests, packages and audits every push; GitHub gets the released
versions.

## Development and issues

Shale is developed on Forgejo: <https://git.home.rabalyn.de/raba/shale>. Branches, pull requests
and the **issues, which are the one list of open work**, live there; GitHub only builds and hosts the
releases ([ADR-0013](docs/decisions/0013-forgejo-and-issue-tracking.md), FORGE rules in
[engineering.md](docs/spec/engineering.md)). Checks run on the Forgejo runner
(`.forgejo/workflows/ci.yml`); GitHub builds only release tags (BUILD-012).

## Build and run

```sh
cargo build --release
./target/release/shale                    # opens the last vault, or the start screen
./target/release/shale --vault ~/Notes    # opens a folder as vault
```

You need a Rust toolchain (1.85 or newer) and the usual desktop libraries (Wayland or X11, plus
OpenGL/Vulkan drivers).

Other options:

- `--screenshot out.png` starts with a temporary demo vault, draws about 120 frames, saves a
  picture of the window and exits. Automated checks use it.
- `--view tasks|graph|drawing|reading|calendar|mail|social|social-import` (with `--screenshot`) chooses which
  view to capture; `mail` uses a made-up mail store (no server, no real mail), `social` and
  `social-import` made-up posts; `open:<file>` opens a file of the vault, `read-only:<file>.canvas`
  shows a canvas without the question, and `convert:<file>.canvas` converts it first.

## Keyboard

| Keys | Action |
|---|---|
| Ctrl+N | New note |
| Ctrl+O | Quick switcher (find or create a note) |
| Ctrl+E | Switch between editing and reading view |
| Ctrl+Shift+F | Search all notes |
| Ctrl+H / Ctrl+L | Keyboard into the left sidebar / back into the editor |
| Ctrl+Shift+M | Mail view. There, with no text field in use: j / k next / previous message, Enter opens, Space marks, Ctrl+J / Ctrl+K next / previous folder, / search, i import to vault, Esc back; u read, s or f flag, d delete, m move; **c compose, r reply, Shift+R reply all, Shift+F forward**. In a new mail: Ctrl+Enter sends, Esc closes (it asks whether to keep the draft) |
| Ctrl+Shift+B | File list with the keyboard in it: j / k next / previous file, Ctrl+J / Ctrl+K next / previous folder, Space opens or closes a folder, Enter opens, Esc back |
| ↓ or Enter in the search box | Browse the results: j / k next / previous hit, Ctrl+J / Ctrl+K next / previous file, Enter opens the note at the hit, Esc back to the box |
| Ctrl+S | Save now (notes also save automatically) |
| Ctrl+W | Close tab |
| Alt+← / Alt+→ | Back / forward |
| Ctrl+, | Settings |
| Click on a link | Open it (or create it if it's red) |
| Ctrl+click on a link | Put the cursor in it so you can edit it (you can swap this in Settings) |
| Middle-click on a link | Open it in a new tab |

With **Vim-style editing** turned on in Settings, a note you go to is in normal mode:
h j k l, w b, W B, 0 $, gg G move; `:12` goes to line 12; `/text` searches, n / N go to the next /
previous hit; i a I A o O start typing and Esc stops; x deletes a character; v / V select (the
motions extend the selection), then d cuts and y copies to the clipboard; p pastes it. The
status bar shows the mode.

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
tests pass with a note.

See [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md) for what is done, what is tested and what is
still missing. What Shale guarantees is in the specs ([docs/spec/](docs/spec/README.md)), and
why it is built that way is in the decision records ([docs/decisions/](docs/decisions/README.md)).
