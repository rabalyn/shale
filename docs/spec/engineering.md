# Spec: Build, quality and performance (BUILD, PERF, JOB)

**IS NOW** = true today (⚠ = known gap) · **SHOULD BE** = agreed target.
Tests are named `file.rs::test_fn`. Format: [ADR-0001](../decisions/0001-spec-driven-development.md). Decisions: [ADR-0006](../decisions/0006-engineering-baseline.md).

## Build and release (BUILD)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| BUILD-001 | `cargo build`, `cargo build --all-targets` and `cargo build --release` produce zero warnings. Checked by hand only, CI does not enforce it. | IS NOW ⚠ | — | manual (`grep '^warning'`) |
| BUILD-002 | Every push to `main` builds on Ubuntu 22.04, runs `cargo test` and updates the `latest` pre-release. | IS NOW | `.github/workflows/build.yml` | CI |
| BUILD-003 | A `v*` tag builds a release only if the tag equals `Cargo.toml`'s version and `CHANGELOG.md` has notes for it. | IS NOW | `build.yml`, `.github/release-notes.sh` | CI |
| BUILD-004 | CI also runs on pull requests, with `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`; actions are pinned by SHA. | SHOULD BE | — | — |
| BUILD-005 | The supported platform is stated: Linux (Wayland and X11) is supported and tested in CI; other systems are unsupported. | SHOULD BE | README | OPEN-007 |
| BUILD-006 | Crates come from crates.io; anything vendored has a written reason, and `cargo audit` runs in CI. ⚠ Today 22 crates are vendored only because an old offline sandbox lacked them. | SHOULD BE | `Cargo.toml` `[patch.crates-io]` | — |
| BUILD-008 | `cargo test` also runs against a real vault: `notes/` next to `Cargo.toml` (git-ignored, kept by hand) or the folder in `SHALE_TEST_VAULT`. Each test works on a copy and checks that the original is unchanged; without such a folder the tests pass with a note (CI). It checks that every file is listed and readable, every note parses, renders and exports to PDF, links agree with backlinks and the graph, every drawing opens and saves without loss, tasks toggle cleanly, and renaming the most linked notes keeps their links working. | IS NOW | `notes_vault_tests.rs` | all 6 tests in `notes_vault_tests.rs` |
| BUILD-007 | `--screenshot <png>` with `--view` and `--keys` runs a temporary demo vault for real-window checks and documentation screenshots. | IS NOW | `main.rs`, `app.rs` | used by checks |

## Background jobs (JOB)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| JOB-001 | Slow work (PDF rendering, CalDAV, reading drops, tag learning) runs on background threads and never blocks the window. | IS NOW | `jobs` (threads) in `pdfview`, `cal_store`, `cal_dialog`, `app/drops` | `pdfview.rs::background_rendering_becomes_ready`, `cal_store.rs::caldav_source_loads_in_the_background` |
| JOB-002 | Every background job ends with a result or an error shown to the user, even if it panics: all jobs start through `jobs::run` / `jobs::spawn`, which catch the panic and deliver it as an error message. | IS NOW | `jobs.rs`; used by `pdfview`, `cal_store`, `cal_dialog`, `app/drops` | `jobs.rs::a_panicking_job_arrives_as_an_error`, `jobs.rs::a_normal_job_returns_its_value_and_notifies`, `jobs.rs::deliver_is_called_once_after_a_panic`, `cal_store.rs::a_panicking_save_job_ends_with_an_error` |

## Performance (PERF)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| PERF-001 | The task list, open-task count and backlinks of the open note are computed once per vault change, not every frame. ⚠ Today `all_tasks()` and `backlinks()` run every frame (`layout.rs:611, 620`). | SHOULD BE | `vault.generation` | — |
| PERF-002 | A rescan of a vault with 10,000 files takes well under the 3 s scan interval. ⚠ Today the rescan has an O(n²) check (`vault.rs:120`) and `Vault::file` is a linear search. | SHOULD BE | `Vault::rescan` | — |
| PERF-003 | The graph view is usable up to a few hundred notes (O(n²) physics). | IS NOW | `graph.rs` | untested |
