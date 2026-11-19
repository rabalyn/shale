# Dataflow diagrams

These diagrams show how data moves through Shale where it crosses a boundary (network, disk or an index, an external program). They are not drawn by hand: a test runs the flow, records the `flow.<flow>.<step>` tracing spans of the code and renders the sequence diagram, so a diagram can only show what the code does (DEV-026, DEV-027).

To regenerate a diagram after a change to its flow, run its driving test (the table below names it) with `SHALE_UPDATE_DATAFLOW=1`, and commit the result. A test in `tests/dataflow_docs.rs` checks that every generated block is listed here and every listed flow has its block.

One row per flow, one row per line; the flow is the name in the begin marker of its block.

| Flow | Doc | Driving test | Drift checked by |
| --- | --- | --- | --- |
| mail_sync | [mail-sync.md](mail-sync.md) | `mail/env_tests.rs::mail_env_password_account_syncs_end_to_end_and_changes_nothing_on_the_server` | mail-env suite (land.sh, when mail code changes) |
| git_sync | [git-sync.md](git-sync.md) | `gitsync/tests.rs::dev_029_dataflow_of_a_first_sync_a_pull_and_an_incremental_sync_with_lfs` | every `cargo test` (needs git-lfs, else skipped) |
| git_lfs_download | [git-lfs-download.md](git-lfs-download.md) | `gitsync/tests.rs::dev_029_dataflow_of_an_lfs_download_on_demand` | every `cargo test` (needs git-lfs, else skipped) |
| social_mastodon | [social-mastodon.md](social-mastodon.md) | `social/dataflow_tests.rs::dev_029_dataflow_of_a_mastodon_sign_in_and_timeline` | every `cargo test` (needs curl, else skipped) |
| social_bluesky | [social-bluesky.md](social-bluesky.md) | `social/dataflow_tests.rs::dev_029_dataflow_of_a_bluesky_sign_in_and_timeline` | every `cargo test` (needs curl, else skipped) |
| pdf_export | [pdf-export.md](pdf-export.md) | `pdf/book_tests.rs::dev_029_dataflow_of_a_note_export` | every `cargo test` |
| book_export | [book-export.md](book-export.md) | `pdf/book_tests.rs::dev_029_dataflow_of_a_book_export` | every `cargo test` |
| import_docx | [import-docx.md](import-docx.md) | `import/dataflow_tests.rs::dev_030_a_word_import_is_a_dataflow_diagram` | every `cargo test` |
| import_thunderbird | [import-thunderbird.md](import-thunderbird.md) | `import/tb_store/dataflow_tests.rs::dev_030_a_thunderbird_lookup_is_a_dataflow_diagram` | every `cargo test` |
| import_mail_text | [import-mail-text.md](import-mail-text.md) | `import/dataflow_tests.rs::dev_030_an_imported_email_is_a_dataflow_diagram` | every `cargo test` |
| import_programs_files | [import-programs.md](import-programs.md) | `import/from_programs/dataflow_tests.rs::dev_030_a_drop_from_another_program_is_a_dataflow_diagram` | every `cargo test` |
| import_programs_web | [import-programs.md](import-programs.md) | `import/from_programs/dataflow_tests.rs::dev_030_a_drop_from_another_program_is_a_dataflow_diagram` | every `cargo test` |
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
