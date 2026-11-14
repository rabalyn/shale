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
- **Linked notes**: backlinks, unlinked mentions, outgoing links, outline, a local graph per note
  and a graph of the whole vault.

See **[docs/FEATURES.md](docs/FEATURES.md)** for all features with screenshots.

## Download

Released versions are on the [Releases page](https://github.com/rabalyn/shale/releases), each
with its notes from [CHANGELOG.md](CHANGELOG.md): download `shale-<version>-linux-x86_64.tar.gz`,
unpack it and run `./shale/shale`. The newest build of `main` (between releases) is the
[latest build](https://github.com/rabalyn/shale/releases/tag/latest). Both are made by GitHub
Actions (`.github/workflows/build.yml`).

### Making a release

1. Raise `version` in `Cargo.toml` ([semver](https://semver.org/): before 1.0, new features raise
   the middle number, fixes the last one) and run `cargo build` (updates `Cargo.lock`).
2. Move the notes from `## [Unreleased]` in `CHANGELOG.md` to a new `## [x.y.z] - date` section.
3. Commit, then run `sh make-release.sh`: it tags the commit `vx.y.z` and pushes. GitHub builds it
   and creates the release page with that section as its notes (the build stops if the tag and
   `Cargo.toml` disagree or the changelog has no notes for the version).

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
- `--view tasks|graph|drawing|reading` (with `--screenshot`) chooses which view to capture.

## Keyboard

| Keys | Action |
|---|---|
| Ctrl+N | New note |
| Ctrl+O | Quick switcher (find or create a note) |
| Ctrl+E | Switch between editing and reading view |
| Ctrl+Shift+F | Search all notes |
| Ctrl+H / Ctrl+L | Previous / next note in the folder (in the file list's sort order) |
| Ctrl+S | Save now (notes also save automatically) |
| Ctrl+W | Close tab |
| Alt+← / Alt+→ | Back / forward |
| Ctrl+, | Settings |
| Click on a link | Open it (or create it if it's red) |
| Ctrl+click on a link | Put the cursor in it so you can edit it (you can swap this in Settings) |
| Middle-click on a link | Open it in a new tab |

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
