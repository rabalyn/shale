# Mail specs (MAIL)

**IS NOW** = true today (⚠ = known gap) · **SHOULD BE** = agreed target.
Tests are named `file.rs::test_fn`. Format: [ADR-0001](../decisions/0001-spec-driven-development.md). Decision: [ADR-0007](../decisions/0007-mail-client.md).
Phase 1a covers the store, the read-only sync and their tests; there is no mail window yet (phase 1b) and no OAuth2 sign-in (phase 1c).

## Local store (MAIL-00x, MAIL-01x)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| MAIL-001 | The mail store lives in one folder (`~/.local/share/shale/mail` by default, another one for tests) that only its owner can enter (mode 0700). | IS NOW | `mail_store.rs` | `mail_store.rs::the_store_folder_is_private` |
| MAIL-002 | Each message is a plain `.eml` file at `<account>/<folder>/<uidvalidity>/<uid>.eml`, written atomically through `fsutil`; the folder name is recoverable from the path. | IS NOW | `mail_store.rs` | `mail_store.rs::folder_names_are_safe_on_disk_and_come_back`, `mail_store.rs::a_message_is_a_file_at_the_documented_path_and_then_a_row` |
| MAIL-003 | A message's index row is written only after its `.eml` file is on disk, so a crash leaves a file without a row, never a row without a file. | IS NOW | `mail_store.rs` | `mail_store.rs::a_message_is_a_file_at_the_documented_path_and_then_a_row`, `mail_store.rs::a_file_without_a_row_is_indexed_at_the_next_start` |
| MAIL-004 | The index (`index.sqlite`) uses WAL and `synchronous=FULL`, has a schema version table, and every migration is tested; a file from a newer version is refused, not changed. | IS NOW | `mail_db.rs` | `mail_db.rs::migration_1_creates_the_tables_and_is_repeatable`, `mail_db.rs::a_newer_index_is_refused_and_left_alone`, `mail_db.rs::the_writer_uses_wal_and_full_sync` |
| MAIL-005 | One writer thread owns the index connection and commits each batch in one transaction; a batch that fails or is cut off leaves no partial rows. | IS NOW | `mail_store.rs`, `mail_db.rs` | `mail_db.rs::a_batch_cut_off_in_the_middle_leaves_nothing`, `mail_store.rs::a_batch_that_dies_in_the_writer_leaves_no_partial_rows` |
| MAIL-006 | At start the index is checked (`PRAGMA quick_check`); a failing file is renamed to `index.sqlite.broken-<date>` (never deleted) and replaced by the newest backup that passes `quick_check`, else by a rebuild from the `.eml` files with the same message count. | IS NOW | `mail_store.rs` | `mail_store.rs::a_damaged_index_is_renamed_and_rebuilt_from_the_files`, `mail_store.rs::a_damaged_index_is_restored_from_the_newest_good_backup` |
| MAIL-007 | At start, `.eml` files without a row are indexed, and rows whose file is gone are marked for refetch (`missing_file`) and fetched again by the next sync. | IS NOW | `mail_store.rs` | `mail_store.rs::a_file_without_a_row_is_indexed_at_the_next_start`, `mail_store.rs::a_row_without_a_file_is_marked_for_refetch_and_unmarked_when_the_file_returns` |
| MAIL-008 | After a completed sync, at most once a day, the index is copied with `VACUUM INTO index.backup-<date>.sqlite`; the newest 3 are kept (older ones are deleted: they are copies of a cache that the files can rebuild). | IS NOW | `mail_store.rs` | `mail_store.rs::backups_are_made_once_a_day_and_the_newest_three_are_kept`, `mail_store.rs::a_damaged_index_is_restored_from_the_newest_good_backup` |
| MAIL-009 | Phase 1 never deletes or moves a `.eml` file; a message deleted on the server only gets `gone_on_server` in its row. | SHOULD BE | `mail_store.rs`, `imap_sync.rs` | — |
| MAIL-010 | Search (FTS5) covers subject, from, to, cc, body text and attachment names, ignores case and accents (`Müller` finds `muller`), and returns newest first. | IS NOW | `mail_db.rs` | `mail_db.rs::search_ignores_case_and_accents_and_sorts_newest_first`, `mail_db.rs::attachment_names_and_cc_are_searchable` |
| MAIL-011 | Search text never reaches the FTS5 query language raw: words and "phrases" are quoted, so `"`, `*`, `NEAR`, `AND`, `-` and `:` in the input are plain text and never an error. | IS NOW | `mail_db.rs` | `mail_db.rs::hostile_search_text_is_plain_text`, `mail_db.rs::the_query_builder_quotes_everything` |
| MAIL-012 | `from:`, `to:`, `cc:`, `subject:` and `body:` limit a search term to that field. | IS NOW | `mail_db.rs` | `mail_db.rs::field_prefixes_limit_a_term_to_that_field` |
| MAIL-013 | The list of a folder is sorted by date, newest first, in pages (limit and offset), and shows only the folder's current UIDVALIDITY. | IS NOW | `mail_db.rs` | `mail_db.rs::the_list_is_newest_first_in_pages_and_shows_the_current_uidvalidity_only`, `mail_store.rs::rebuilding_adopts_the_newest_uidvalidity_and_keeps_old_ones_unlisted` |
| MAIL-014 | A message that cannot be read is stored as raw `.eml` with a "could not parse" row; random bytes never make storing or indexing panic. | IS NOW | `mail_store.rs` | `mail_store.rs::a_message_that_cannot_be_read_is_stored_raw_with_a_could_not_parse_row`, `mail_store.rs::random_and_hostile_bytes_never_panic_the_parse_and_index_path` |
| MAIL-015 | SQLite is the bundled build with FTS5 (no system library needed). | IS NOW | `Cargo.toml` | `mail_db.rs::sqlite_is_bundled_with_fts5` |

## Accounts and connection (MAIL-02x)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| MAIL-020 | An account has host, port, user, a sign-in kind (password or OAuth2) and a security mode that is implicit TLS or mandatory STARTTLS; there is no plain-text mode, and no credentials are sent before TLS. | SHOULD BE | `config.rs`, `imap_client.rs` | — |
| MAIL-021 | Account passwords (and later refresh tokens) are kept in the system keyring through `secrets.rs`, never in the settings file; CalDAV passwords behave as before. | IS NOW | `secrets.rs` | `secrets.rs::mail_secrets_have_their_own_entries`, `secrets.rs::passwords_go_to_the_keyring_and_come_back` |
| MAIL-022 | Server certificates are checked by the operating system's verifier (`rustls-platform-verifier`); the production path has no way to skip or loosen the check. | SHOULD BE | `imap_client.rs` | — |
| MAIL-023 | The client sends only read-only commands: `EXAMINE` (never `SELECT`), `UID SEARCH`, `UID FETCH` with `BODY.PEEK[...]` (never `BODY[...]`, `RFC822`), `LIST`, `CAPABILITY`, sign-in, `STARTTLS`, `LOGOUT`; any other command is refused before it is sent. | SHOULD BE | `imap_client.rs` | — |
| MAIL-024 | The client refuses literals bigger than its limit without reading them into memory, lines longer than its limit, and gives up on a silent server after a timeout. | SHOULD BE | `imap_client.rs` | — |
| MAIL-025 | Sign-in goes through an authenticator hook (`PLAIN` for passwords), so XOAUTH2 can be added without changing the client. | SHOULD BE | `imap_client.rs` | — |

## Sync (MAIL-03x)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| MAIL-030 | A sync lists the folders with their special-use flags; Gmail's `\All` is off by default. | SHOULD BE | `imap_sync.rs` | — |
| MAIL-031 | Trash and Junk (special-use, else the usual names) are never synced automatically; their list and single messages are fetched only on request (`fetch_folder_listing`, `fetch_message`). | SHOULD BE | `imap_sync.rs` | — |
| MAIL-032 | The first sync of a folder fetches the messages of the last 3 years, newest first, in batches, and resumes from what the index holds after an interruption. | SHOULD BE | `imap_sync.rs` | — |
| MAIL-033 | A later sync fetches only messages with a UID from the stored UIDNEXT on. | SHOULD BE | `imap_sync.rs` | — |
| MAIL-034 | If a folder's UIDVALIDITY changed, the folder is fetched again into a new UIDVALIDITY folder; the old files stay. | SHOULD BE | `imap_sync.rs` | — |
| MAIL-035 | Flag changes and deletions are found with `CONDSTORE` (changed flags only) where offered, else by comparing `UID FETCH FLAGS` and `UID SEARCH ALL`. | SHOULD BE | `imap_sync.rs` | — |
| MAIL-036 | Up to 4 connections per account sync different folders at once; parsing and file writing happen in those workers, one writer thread commits to the index. | SHOULD BE | `imap_sync.rs` | — |
| MAIL-037 | A sync reports progress (folder, done, total, errors) over a channel and stops between batches when cancelled; an error in one folder does not stop the others. | SHOULD BE | `imap_sync.rs` | — |
| MAIL-038 | A message bigger than the size limit (default 200 MB) is not stored; its row says "too large" so it is not downloaded again and again. | SHOULD BE | `imap_sync.rs` | — |
| MAIL-039 | The sync job ends with a summary even if it panics (`jobs`), and a backup is made only after a sync that finished without error. | SHOULD BE | `imap_sync.rs` | — |
