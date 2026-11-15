# Spec: Build, quality and performance (BUILD, PERF, JOB)

**IS NOW** = true today (⚠ = known gap) · **SHOULD BE** = agreed target.
Tests are named `file.rs::test_fn`. Format: [ADR-0001](../decisions/0001-spec-driven-development.md). Decisions: [ADR-0006](../decisions/0006-engineering-baseline.md).

## Build and release (BUILD)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| BUILD-001 | `cargo build`, `cargo build --all-targets` and `cargo build --release` produce zero warnings, `cargo clippy --all-targets -- -D warnings` is clean and `cargo fmt --check` passes (`rustfmt.toml` matches the long-line style). CI builds with `RUSTFLAGS=-D warnings`. `Cargo.toml` `[lints]` also denies `unsafe_op_in_unsafe_fn`, `unused_must_use`, `dbg!`, `todo!` and `unimplemented!`. | IS NOW | `Cargo.toml` `[lints]`, `rustfmt.toml`, `.forgejo/workflows/ci.yml`, `.github/workflows/build.yml` | CI (fmt, clippy, build, test steps) |
| BUILD-002 | Every push (any branch) and pull request on the homelab Forgejo (git remote `origin`) runs in a `rust:1.100.0-bookworm` container: `cargo fmt --check`, clippy, `cargo build --release`, `cargo test --release` and the packaging of BUILD-010; nothing is published from there. Its steps are plain shell (no JavaScript actions: the Rust image has no Node and the runner may use Podman). Until 2026-11-15 this ran on GitHub for every push to `main` and updated a rolling `latest` pre-release; that pre-release is not updated any more. | IS NOW | `.forgejo/workflows/ci.yml` | CI (Forgejo) |
| BUILD-003 | A `v*` tag builds a release only if the tag equals `Cargo.toml`'s version and `CHANGELOG.md` has notes for it; Forgejo checks the same for the tag, without publishing. | IS NOW | `.github/workflows/build.yml`, `.forgejo/workflows/ci.yml`, `.github/release-notes.sh` | CI |
| BUILD-004 | CI runs on pull requests too (on Forgejo), with `cargo fmt --check` and `cargo clippy --all-targets --locked -- -D warnings`; GitHub actions are pinned by commit SHA (the tag is in a comment); the Forgejo workflow uses no actions. | IS NOW | `.forgejo/workflows/ci.yml`, `.github/workflows/build.yml` | CI |
| BUILD-005 | The supported platform is stated: Linux (Wayland and X11) is supported and tested in CI; other systems are unsupported. | SHOULD BE | README | OPEN-007 |
| BUILD-006 | Crates come from crates.io; anything vendored has a written reason (`cargo audit` runs in CI since BUILD-009). ⚠ Today 22 crates are vendored only because an old offline sandbox lacked them. | SHOULD BE | `Cargo.toml` `[patch.crates-io]` | — |
| BUILD-008 | `cargo test` also runs against a real vault: `notes/` next to `Cargo.toml` (git-ignored, kept by hand) or the folder in `SHALE_TEST_VAULT`. Each test works on a copy and checks that the original is unchanged; without such a folder the tests pass with a note (CI). It checks that every file is listed and readable, every note parses, renders and exports to PDF, links agree with backlinks and the graph, every drawing opens and saves without loss, tasks toggle cleanly, and renaming the most linked notes keeps their links working. | IS NOW | `notes_vault_tests.rs` | all 6 tests in `notes_vault_tests.rs` |
| BUILD-007 | `--screenshot <png>` with `--view` and `--keys` runs a temporary demo vault for real-window checks and documentation screenshots. | IS NOW | `main.rs`, `app.rs` | used by checks |
| BUILD-009 | A separate `audit` job (not a step of the build job) runs `cargo audit` on `Cargo.lock`: on Forgejo on every push and pull request and weekly (`on: schedule`, where only the audit runs), and on GitHub for every release tag. A new advisory fails only that job, never the build or the release. | IS NOW | `.forgejo/workflows/ci.yml`, `.github/workflows/build.yml` | CI (cannot be run locally) |
| BUILD-010 | The build writes `shale-linux-x86_64.tar.gz.sha256` in `sha256sum` format and checks it with `sha256sum -c` (Forgejo, every push); the tagged GitHub release uploads `shale-<version>-linux-x86_64.tar.gz` with `shale-<version>-linux-x86_64.tar.gz.sha256` naming the versioned file, checked the same way before the upload, and the tarball and its checksum as a workflow artifact. | IS NOW | `.forgejo/workflows/ci.yml`, `.github/workflows/build.yml` | CI |
| BUILD-011 | The release notes of a tagged release and the README say how to check the download (`sha256sum -c <file>.sha256`). The checksum guards against a damaged download only; releases are not signed (OPEN-018). | IS NOW | `build.yml`, `README.md` | CI |
| BUILD-012 | GitHub (`rabalyn/shale`, remote `github`) is only for publishing: its workflow runs for `v*` tags (and by hand, `workflow_dispatch`, which builds without publishing), builds and tests the tagged commit again on Ubuntu 22.04 and publishes the release. `make-release.sh` pushes the commit and the tag to Forgejo and then to GitHub; day-to-day pushes go to Forgejo only. | IS NOW | `.github/workflows/build.yml`, `make-release.sh` | CI |

## Background jobs (JOB)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| JOB-001 | Slow work (PDF rendering, CalDAV, reading drops, tag learning) runs on background threads and never blocks the window. | IS NOW | `jobs` (threads) in `pdfview`, `cal_store`, `cal_dialog`, `app/drops`, `social/state`, `social/images`, `social/accounts_ui` | `pdfview.rs::background_rendering_becomes_ready`, `cal_store.rs::caldav_source_loads_in_the_background` |
| JOB-002 | Every background job ends with a result or an error shown to the user, even if it panics: all jobs start through `jobs::run` / `jobs::spawn`, which catch the panic and deliver it as an error message. | IS NOW | `jobs.rs`; used by `pdfview`, `cal_store`, `cal_dialog`, `app/drops`, `social/*` | `jobs.rs::a_panicking_job_arrives_as_an_error`, `jobs.rs::a_normal_job_returns_its_value_and_notifies`, `jobs.rs::deliver_is_called_once_after_a_panic`, `cal_store.rs::a_panicking_save_job_ends_with_an_error` |

## Performance (PERF)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| PERF-001 | The task list, open-task count and backlinks of the open note are computed once per vault change, not every frame. ⚠ Today `all_tasks()` and `backlinks()` run every frame (`layout.rs:611, 620`). | SHOULD BE | `vault.generation` | — |
| PERF-002 | A rescan of a vault with 10,000 files takes well under the 3 s scan interval. ⚠ Today the rescan has an O(n²) check (`vault.rs:120`) and `Vault::file` is a linear search. | SHOULD BE | `Vault::rescan` | — |
| PERF-003 | The graph view is usable up to a few hundred notes (O(n²) physics). | IS NOW | `graph.rs` | untested |
