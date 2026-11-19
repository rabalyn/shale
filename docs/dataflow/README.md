# Dataflow diagrams

These diagrams show how data moves through Shale where it crosses a boundary (network, disk or an index, an external program). They are not drawn by hand: a test runs the flow, records the `flow.<flow>.<step>` tracing spans of the code and renders the sequence diagram, so a diagram can only show what the code does (DEV-026, DEV-027).

To regenerate a diagram after a change to its flow, run its driving test (the table below names it) with `SHALE_UPDATE_DATAFLOW=1`, and commit the result. A test in `tests/dataflow_docs.rs` checks that every generated block is listed here and every listed flow has its block.

One row per flow, one row per line; the flow is the name in the begin marker of its block.

| Flow | Doc | Driving test | Drift checked by |
| --- | --- | --- | --- |
| mail_sync | [mail-sync.md](mail-sync.md) | `mail/env_tests.rs::mail_env_password_account_syncs_end_to_end_and_changes_nothing_on_the_server` | mail-env suite (land.sh, when mail code changes) |
| import_docx | [import-docx.md](import-docx.md) | `import/dataflow_tests.rs::dev_030_a_word_import_is_a_dataflow_diagram` | every `cargo test` |
| import_thunderbird | [import-thunderbird.md](import-thunderbird.md) | `import/tb_store/dataflow_tests.rs::dev_030_a_thunderbird_lookup_is_a_dataflow_diagram` | every `cargo test` |
| import_mail_text | [import-mail-text.md](import-mail-text.md) | `import/dataflow_tests.rs::dev_030_an_imported_email_is_a_dataflow_diagram` | every `cargo test` |
| import_programs | [import-programs.md](import-programs.md) | `import/from_programs/dataflow_tests.rs::dev_030_a_drop_from_another_program_is_a_dataflow_diagram` | every `cargo test` |
