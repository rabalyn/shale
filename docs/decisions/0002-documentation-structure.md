# ADR-0002: One job per document

- **Status**: Proposed
- **Date**: 2026-11-14

## Context

The review of the README and docs on 2026-11-14 found these problems:

| Where | Problem |
|---|---|
| `IMPLEMENTATION.md` "Missing / next steps" | Lists features that are done: drag and drop in the file tree, images inside the editor, arrow binding, text in shapes (`containerId`), the hand-drawn font, images in notes. |
| `IMPLEMENTATION.md:59` | The dependency list names only eframe, serde, dirs, image and libc. It leaves out hayro (vendored, 22 crates), roxmltree, ttf-parser, miniz_oxide, winit and wayland-*. |
| `IMPLEMENTATION.md:6` | "Last updated … thirteenth round", although rounds up to 18 are described. |
| `IMPLEMENTATION.md:90-91`, `publish-to-github.sh` | Says there is no git and describes `gitlite.py`. Git is used now; the GitHub repository exists. |
| `IMPLEMENTATION.md` "Tested" | "130 tests"; there are 179. "Not covered: the file tree" is now partly covered. |
| `IMPLEMENTATION.md` "Known issues" | "Rename is offered for folders, but links are not rewritten." This is still true, but `move_into` now handles folders, so the two disagree. |
| README vs FEATURES | Two keyboard tables that differ: Ctrl+S, Ctrl+W and the mouse rows are only in the README. |
| README | Says "Linux desktop", while `Cargo.toml` has macOS dependencies and `lists.rs` has Windows code. There is no stated platform policy. |

The cause is that one file (`IMPLEMENTATION.md`) does four jobs: architecture, history, test
inventory and backlog. History is also kept in `CHANGELOG.md`.

## Decision

| Document | Its only job | Audience |
|---|---|---|
| `README.md` | What Shale is, download, build, run, release steps; links onward | users, contributors |
| `docs/FEATURES.md` | Features with screenshots, and **the** keyboard reference | users |
| `CHANGELOG.md` | What changed per version (Keep a Changelog) | users |
| `docs/spec/*.md` | What Shale guarantees (IS NOW / SHOULD BE) | developers |
| `docs/decisions/NNNN-*.md` | Why a decision was made | developers |
| `docs/spec/open-points.md` | Undecided questions and the backlog | developers |
| `docs/IMPLEMENTATION.md` → rename to `docs/ARCHITECTURE.md` | Module map, threading model, external programs, dependencies, development notes (no-warnings rule, linker shim) | developers |

Migration, in one change:

1. Move the "round N" log and the long "Done" list out of `IMPLEMENTATION.md`. Anything still
   useful goes into a spec rule or into `CHANGELOG.md`; git keeps the rest.
2. Move "Missing / next steps" and "Known issues" to `open-points.md`, after removing what is done.
3. Keep one keyboard table, in `FEATURES.md`, and link to it from the README.
4. Fix the dependency list and the test count. Better still, drop the count, because it drifts.
5. Delete or archive `publish-to-github.sh` and the sandbox notes.

## Consequences

- Each fact has one home. A reviewer knows which file a change must touch: a behaviour change
  touches a spec and the changelog, a new feature touches FEATURES, a structure change touches
  ARCHITECTURE.
- One-time cleanup effort; links to `IMPLEMENTATION.md` must be updated (README "Tests" section).
