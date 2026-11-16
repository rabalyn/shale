# ADR-0006: Engineering baseline — CI gates, background jobs, external programs, structure

- **Status**: Proposed
- **Date**: 2026-11-14
- **Specs**: SEC-005, SEC-006, PERF-001…PERF-003, BUILD-001…BUILD-005

## Context

What works well and should be kept:

- A strong test culture: 179 tests, headless egui UI tests that click real controls,
  real-window runs with `--screenshot --keys`, a pretend CalDAV server driven through real
  `curl`, and a UTF-8 torture test.
- The zero-warnings rule.
- Few dependencies: a pure-Rust PDF stack and no date library.

The review found these gaps:

- **CI** (`.github/workflows/build.yml`):
  - It runs only on pushes to `main`, never on pull requests.
  - It runs no `clippy` and no `fmt --check`, and does not build with `-D warnings`, so
    "zero warnings" relies on discipline.
  - It runs only on Linux, although macOS and Windows code exists.
  - Actions are pinned by tag, not by SHA, while the job has `contents: write`.
- **Vendored crates**: `vendor/` holds 22 crates (about 30 MB), added only because an offline
  sandbox lacked them. They get no upstream security fixes.
- **Background jobs**: `thread::spawn` and an mpsc channel, with no `catch_unwind`. If a job
  panics, for example hayro on a malformed PDF attachment, its result never arrives and the
  spinner runs forever (`pdfview.rs:77-80`, `calendar/store.rs:64-67`).
- **External programs**: `xdg-open` gets calendar event URLs from a remote server without a
  scheme check (`calendar/ui.rs:671` → `lists.rs:345`), and any non-note file, including untrusted
  mail attachments (`app.rs:438`). Every command is started with an argument list, never a
  shell, which is good.
- **Performance**:
  - Every frame computes `all_tasks()` (clones and sorts every task, `layout.rs:620`) and
    `backlinks()` (`layout.rs:611`).
  - `Vault::rescan` has an O(n²) check (`vault.rs:120`), and `Vault::file` is a linear search.
- **Structure**:
  - `ShaleApp` has about 50 fields covering tabs, calendar, drag-and-drop state, tag learning
    jobs and screenshot scripting.
  - `impl ShaleApp` is spread over `app.rs` (1,424 lines), `panels.rs` (1,082) and
    `layout.rs`.
  - 38 modules sit flat in `main.rs`.

## Decision (proposed)

1. **CI gates**: run on `push` **and** `pull_request` (since 2026-11-15 on the homelab Forgejo,
   `.forgejo/workflows/ci.yml`; GitHub builds only the `v*` release tags, BUILD-012):
   - `cargo fmt --check`
   - `cargo clippy --all-targets -- -D warnings`
   - `cargo test`
   - Pin actions by SHA.
   - State the platform policy in the README: Linux supported and tested; other systems
     unsupported.
2. **Vendor**: try to drop `vendor/` and use crates.io. If something must stay vendored, record
   why in `vendor/README.md`, and run `cargo audit` in CI.
3. **Background jobs** run through one helper, `jobs::spawn`, which wraps `catch_unwind`. A
   panic arrives as an error result, so every spinner ends with either a result or a message.
4. **External opening** goes through one function:
   - Only `http`, `https` and `mailto` URLs are passed on.
   - Files are opened only from inside the vault.
   - Executable files and `.desktop` files are never opened.
5. **Derived data** (task list, open-task count, backlinks of the open note) is cached by
   `vault.generation`. The rescan uses a `HashSet`, and `Vault::file` uses a map.
6. **Structure**, step by step while working on each area:
   - Feature folders: `calendar/`, `drawing/`, `import/` (mail, tb_store, wl_drop, people,
     tagwords), `pdf/`.
   - Split `ShaleApp` into sub-states (`DragState`, `CalendarState`, `TagLearner`, `Script`)
     owned by the app.
   - No big-bang restructuring.

## Consequences

- PRs get the same checks as `main`, and warnings fail the build instead of relying on
  discipline.
- Restructuring is spread out, so each step stays reviewable.
- Dropping `vendor/` needs network access for builds, which is normal outside the old sandbox.

## Decisions

- proposed decisions and consequences are fine for me, we do it like this.
- when there are additional good practices for rust development to catch errors / warnings / issues we should introduce them aswell. "always run strict" is a MUST in my opinion, it does not hurt.
- can we save as a baseline that read/write functions MUST use a baseline read/write util func of ours, that runs atomic calls and so that we only need to test that behaviour in one spot?