# Mail specs (MAIL)

**IS NOW** = true today (⚠ = known gap) · **SHOULD BE** = agreed target.
Tests are named `file.rs::test_fn`. Format: [ADR-0001](../decisions/0001-spec-driven-development.md). Decision: [ADR-0007](../decisions/0007-mail-client.md).
Phase 1a covers the store, the read-only sync and their tests; there is no mail window yet (phase 1b) and no OAuth2 sign-in (phase 1c).

## Local store (MAIL-00x, MAIL-01x)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| MAIL-001 | The mail store lives in one folder (`~/.local/share/shale/mail` by default, another one for tests) that only its owner can enter (mode 0700). | SHOULD BE | `mail_store.rs` | — |
| MAIL-002 | Each message is a plain `.eml` file at `<account>/<folder>/<uidvalidity>/<uid>.eml`, written atomically through `fsutil`; the folder name is recoverable from the path. | SHOULD BE | `mail_store.rs` | — |
| MAIL-003 | A message's index row is written only after its `.eml` file is on disk, so a crash leaves a file without a row, never a row without a file. | SHOULD BE | `mail_store.rs` | — |
| MAIL-004 | The index (`index.sqlite`) uses WAL and `synchronous=FULL`, has a schema version table, and every migration is tested; a file from a newer version is refused, not changed. | SHOULD BE | `mail_db.rs` | — |
| MAIL-005 | One writer thread owns the index connection and commits each batch in one transaction; a batch that fails or is cut off leaves no partial rows. | SHOULD BE | `mail_store.rs`, `mail_db.rs` | — |
| MAIL-006 | At start the index is checked (`PRAGMA quick_check`); a failing file is renamed to `index.sqlite.broken-<date>` (never deleted) and replaced by the newest backup that passes, else by a rebuild from the `.eml` files with the same message count. | SHOULD BE | `mail_store.rs` | — |
| MAIL-007 | At start, `.eml` files without a row are indexed, and rows whose file is gone are marked for refetch (`missing_file`) and fetched again by the next sync. | SHOULD BE | `mail_store.rs` | — |
| MAIL-008 | After a completed sync, at most once a day, the index is copied with `VACUUM INTO index.backup-<date>.sqlite`; the newest 3 backups that pass `quick_check` are kept. | SHOULD BE | `mail_store.rs` | — |
| MAIL-009 | Phase 1 never deletes or moves a `.eml` file; a message deleted on the server only gets `gone_on_server` in its row. | SHOULD BE | `mail_store.rs`, `imap_sync.rs` | — |
| MAIL-010 | Search (FTS5) covers subject, from, to, cc, body text and attachment names, ignores case and accents (`Müller` finds `muller`), and returns newest first. | SHOULD BE | `mail_db.rs` | — |
| MAIL-011 | Search text never reaches the FTS5 query language raw: words and "phrases" are quoted, so `"`, `*`, `NEAR`, `AND`, `-` and `:` in the input are plain text and never an error. | SHOULD BE | `mail_db.rs` | — |
| MAIL-012 | `from:`, `to:`, `cc:` and `subject:` limit a search term to that field. | SHOULD BE | `mail_db.rs` | — |
| MAIL-013 | The list of a folder is sorted by date, newest first, in pages (limit and offset), and shows only the folder's current UIDVALIDITY. | SHOULD BE | `mail_db.rs` | — |
| MAIL-014 | A message that cannot be read is stored as raw `.eml` with a "could not parse" row; random bytes never make storing or indexing panic. | SHOULD BE | `mail_store.rs` | — |
| MAIL-015 | SQLite is the bundled build with FTS5 (no system library needed). | SHOULD BE | `Cargo.toml` | — |

## Accounts and connection (MAIL-02x)

| ID | Rule | Status | Code | Test |
|---|---|---|---|---|
| MAIL-020 | An account has host, port, user, a sign-in kind (password or OAuth2) and a security mode that is implicit TLS or mandatory STARTTLS; there is no plain-text mode, and no credentials are sent before TLS. | SHOULD BE | `config.rs`, `imap_client.rs` | — |
| MAIL-021 | Account passwords (and later refresh tokens) are kept in the system keyring through `secrets.rs`, never in the settings file; CalDAV passwords behave as before. | SHOULD BE | `secrets.rs` | — |
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
