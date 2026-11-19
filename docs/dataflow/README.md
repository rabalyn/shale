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
| mail_oauth | [mail-oauth.md](mail-oauth.md) | `mail/env_tests.rs::mail_env_oauth_account_signs_in_syncs_renews_its_token_and_a_dead_sign_in_is_said_in_words` | mail-env suite (land.sh, when mail code changes) |
| mail_oauth_renew | [mail-oauth.md](mail-oauth.md) | `mail/env_tests.rs::mail_env_oauth_account_signs_in_syncs_renews_its_token_and_a_dead_sign_in_is_said_in_words` | mail-env suite (land.sh, when mail code changes) |
| mail_send | [mail-send.md](mail-send.md) | `mail/env_tests.rs::mail_env_password_and_oauth_accounts_send_with_an_attachment_and_dovecot_has_one_sent_copy_each` | mail-env suite (land.sh, when mail code changes) |
| mail_ops | [mail-ops.md](mail-ops.md) | `mail/env_tests.rs::mail_env_flag_unflag_move_delete_and_delete_forever_change_dovecot_as_asked_and_nothing_else` | mail-env suite (land.sh, when mail code changes) |
| mail_watch | [mail-watch.md](mail-watch.md) | `mail/env_tests.rs::mail_env_a_flag_that_another_connection_sets_and_removes_shows_in_the_store_within_seconds_through_idle` | mail-env suite (land.sh, when mail code changes) |
| mail_crypto_send | [mail-crypto.md](mail-crypto.md) | `mail/env_tests.rs::mail_env_signed_and_encrypted_openpgp_and_smime_mail_arrives_intact_verifies_decrypts_and_the_sent_copy_decrypts` | mail-env suite (land.sh, when mail code changes) |
| mail_crypto_read | [mail-crypto.md](mail-crypto.md) | `mail/env_tests.rs::mail_env_signed_and_encrypted_openpgp_and_smime_mail_arrives_intact_verifies_decrypts_and_the_sent_copy_decrypts` | mail-env suite (land.sh, when mail code changes) |
| caldav_discover | [caldav-load.md](caldav-load.md) | `calendar/env_tests.rs::caldav_env_discovers_and_loads_the_seeded_calendars` | caldav-env suite (land.sh, when calendar code changes) |
| caldav_load | [caldav-load.md](caldav-load.md) | `calendar/env_tests.rs::caldav_env_discovers_and_loads_the_seeded_calendars` | caldav-env suite (land.sh, when calendar code changes) |
| caldav_edit | [caldav-edit.md](caldav-edit.md) | `calendar/env_tests.rs::caldav_env_creates_edits_and_deletes_an_event` | caldav-env suite (land.sh, when calendar code changes) |
| caldav_move | [caldav-edit.md](caldav-edit.md) | `calendar/env_tests.rs::caldav_env_moves_an_event_to_another_calendar_and_deletes_it` | caldav-env suite (land.sh, when calendar code changes) |
| tex_export | [tex-export.md](tex-export.md) | `pdf/tex/writer_env_tests.rs::tex_env_a_letter_has_its_recipient_subject_and_text` | tex-env suite (land.sh, when TeX code changes) |
| tex_book | [tex-book.md](tex-book.md) | `pdf/tex/book_env_tests.rs::tex_env_a_folder_compiles_as_a_report_with_sections` | tex-env suite (land.sh, when TeX code changes) |
| tex_setup | [tex-setup.md](tex-setup.md) | `pdf/tex/setup_env_tests.rs::tex_env_the_setup_imports_the_cached_archive_takes_a_logo_and_the_details_and_makes_a_test_page_with_the_department` | tex-env suite (land.sh, when TeX code changes) |
