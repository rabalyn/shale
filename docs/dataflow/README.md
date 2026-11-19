# Dataflow diagrams

These diagrams show how data moves through Shale where it crosses a boundary (network, disk or an index, an external program). They are not drawn by hand: a test runs the flow, records the `flow.<flow>.<step>` tracing spans of the code and renders the sequence diagram, so a diagram can only show what the code does (DEV-026, DEV-027).

To regenerate a diagram after a change to its flow, run its driving test (the table below names it) with `SHALE_UPDATE_DATAFLOW=1`, and commit the result. A test in `tests/dataflow_docs.rs` checks that every generated block is listed here and every listed flow has its block.

One row per flow, one row per line; the flow is the name in the begin marker of its block.

| Flow | Doc | Driving test | Drift checked by |
| --- | --- | --- | --- |
| mail_sync | [mail-sync.md](mail-sync.md) | `mail/env_tests.rs::mail_env_password_account_syncs_end_to_end_and_changes_nothing_on_the_server` | mail-env suite (land.sh, when mail code changes) |
| vault_open | [vault-open.md](vault-open.md) | `app.rs::dataflow_vault_open` | every `cargo test` |
| vault_check | [vault-check.md](vault-check.md) | `vault_check.rs::dataflow_vault_check` | every `cargo test` |
| vault_search | [vault-search.md](vault-search.md) | `search_all.rs::dataflow_vault_search` | every `cargo test` |
| recovery_rescue | [recovery.md](recovery.md) | `recovery.rs::dataflow_recovery_rescue` | every `cargo test` |
| recovery_unseen | [recovery.md](recovery.md) | `recovery.rs::dataflow_recovery_unseen` | every `cargo test` |
| recovery_dismiss | [recovery.md](recovery.md) | `recovery.rs::dataflow_recovery_dismiss` | every `cargo test` |
| data_usage_scan | [data-usage.md](data-usage.md) | `data_usage/usage_tests.rs::dataflow_data_usage_scan` | every `cargo test` |
| data_usage_clear | [data-usage.md](data-usage.md) | `data_usage/usage_tests.rs::dataflow_data_usage_clear` | every `cargo test` |
| data_usage_prune | [data-usage.md](data-usage.md) | `data_usage/usage_tests.rs::dataflow_data_usage_prune` | every `cargo test` |
| link_preview | [link-preview.md](link-preview.md) | `link_preview.rs::dataflow_link_preview` | every `cargo test` |
