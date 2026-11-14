# ADR-0007: A built-in mail view (IMAP), and later sending and signing

- **Status**: Proposed
- **Date**: 2026-11-14
- **Specs**: MAIL-001… (to be written before the code, ADR-0001), SEC-006, JOB-002, DATA-004

## Context

Mails get into the vault by dragging them from Thunderbird (IMP-001…IMP-010). Arranging two
windows for every drag is tedious. Shale should show the mail itself, so a mail can be read,
searched and turned into a note without leaving the window.

What Shale already has:

- `mail.rs` reads any RFC 5322 message: encoded headers, MIME, charsets, attachments. It turns
  HTML into plain text, so no web engine, remote images or scripts are ever involved.
- `import.rs` turns a mail into a note with its attachments and people (IMP-003…IMP-005, PPL-002).
- `secrets.rs` keeps CalDAV passwords in the system keyring (Secret Service via `secret-tool`).
- `tb_store.rs` already reads Thunderbird's local folders and `.msf` indexes.

What it does not have: TLS sockets of its own (CalDAV goes through `curl`), a database, or a
panic-safe job runner (JOB-002).

## Decision (proposed)

### 1. Scope by phase

| Phase | What | Writes to the server? |
|---|---|---|
| 0 | Prerequisites: one `fsutil` module that every file write goes through (atomic temp + fsync + rename, ADR-0003/0006, tested once, property-tested), SEC-006 (allowlist for `xdg-open`; attachments open only from a temp copy, never executables), JOB-002 (panic-safe jobs), strict lints | — |
| 1 | **Read-only IMAP**: accounts (password and OAuth2), sync of all folders for the last 3 years, list, reading view, search, "Import to vault" | **No.** Folders are opened with `EXAMINE`, bodies fetched with `BODY.PEEK[]`, so Shale never even marks a mail read |
| 2 | Flags (read, flagged), move, delete (to Trash) | Yes: two-way sync |
| 3 | SMTP: compose, reply, forward, drafts, Sent copy | Yes |
| 4 | OpenPGP and S/MIME: verify and decrypt, then sign and encrypt | — |

Each phase is its own change with its own specs and can ship alone.

### 2. Storage: one `.eml` file per message plus an SQLite index, outside the vault

```
$XDG_DATA_HOME/shale/mail/            (~/.local/share/shale/mail, mode 0700)
  index.sqlite                        accounts, folders, messages, flags, FTS5 search
  <account-id>/<folder-id>/<uidvalidity>/<uid>.eml
```

- **Messages are plain `.eml` files**, written atomically (temp file + rename). `mail.rs` reads
  them as they are; they can be backed up, grepped, or opened by any mail program.
- **The index is a cache.** It holds per folder `UIDVALIDITY`, `UIDNEXT` and `HIGHESTMODSEQ`,
  and per message UID, flags, date, from, to, subject, Message-ID, size and file path. It can be
  rebuilt from the files. Search uses SQLite FTS5 over subject, people and body text (`mail.rs`
  output; attachments' names, not their contents).
- `index.sqlite` is opened in WAL mode; one writer (the sync job), readers in the UI.

**Why not like Thunderbird?** Thunderbird's default store is one mbox file per folder plus a
`.msf` index in the Mork format. mbox needs whole-file rewrites ("compacting") to delete a mail,
and one damaged write can harm a whole folder. Mork is an old, poorly documented format that
Thunderbird itself is moving away from (it has offered maildir as an option for years and
works on an SQLite-based message database). Its *ideas* are sound and kept here: a local
copy per folder, an index that is only a cache, and search through SQLite (Thunderbird's global
search, Gloda, is SQLite full-text search). The layout above is the one the long-lived Unix
clients use: one file per message (maildir: mutt, notmuch, mu, aerc) plus a separate index.

### 3. Sync rules

- Accounts: host, port, user name, implicit TLS on 993 (STARTTLS only if 993 is not offered, and
  then the upgrade is mandatory). No plain-text connection, ever. Password or app password in the
  keyring (`secrets.rs` generalised from CalDAV to "account" entries).
- **First sync**: list folders (special-use flags from `LIST`), then per folder
  `UID SEARCH SINCE <today − 3 years>`, fetch newest first, in batches, resumable after a crash or
  quit. Default: all folders except Gmail's `\All` (it would duplicate every message).
  Per-folder checkboxes in the account settings.
- **Trash and Junk are never synced automatically** (special-use `\Trash` / `\Junk`, else the
  usual names). Their list is fetched only while the user browses them, and a message only when
  it is opened.
- **Later syncs**: if `UIDVALIDITY` changed, the folder is re-fetched. New mail: `UID FETCH
  <UIDNEXT>:*`. Flags and deletions: `CONDSTORE`/`QRESYNC` where offered, else a cheap
  `UID SEARCH ALL` compare. On a timer and on a button; `IDLE` on INBOX later.
- Messages older than the window stay until the user prunes them; nothing is deleted locally
  because it aged out.
- **Parallel work**: per account a small pool of IMAP connections (default 4, the servers' limit
  is often 10–20 per user) fetches different folders at once; parsing and `.eml` writing happen in
  the workers. **One** writer thread owns `index.sqlite` and receives finished messages over a
  channel, committing them in batches. Progress shows in the status bar.

### 3a. Crash safety (ADR-0003: never lose or corrupt data)

- Order per message: write the `.eml` atomically (temp, fsync, rename, fsync the folder), **then**
  insert its row. A crash in between leaves a file without a row, never a row without a file.
- SQLite in WAL mode with `synchronous=FULL`; each batch is one transaction, so a kill mid-batch
  rolls back to the last whole batch. Resuming starts from what the index says was committed.
- On start: `PRAGMA quick_check`. Files without a row are indexed; rows without a file are
  re-fetched.
- **The index can always be rebuilt** from the `.eml` files (UID, folder and UIDVALIDITY are in the
  path; flags come back from the server). A corrupt index is renamed to
  `index.sqlite.broken-<date>` and rebuilt; nothing is deleted.
- **Backups anyway**: after each completed sync, at most once a day, `VACUUM INTO
  index.backup-<date>.sqlite`; the last 3 are kept. A rebuild first tries the newest backup that
  passes `quick_check`, then the files.
- Phase 1 only ever adds files. Removing local copies (a mail deleted on the server) moves them to
  `mail/.trash/<date>/` and is the user's action, not automatic.

### 4. Libraries

The project keeps dependencies few, but IMAP needs real TLS sockets, and `curl` cannot hold a
connection or run `IDLE`. Proposed:

- **TLS**: `rustls` with `rustls-platform-verifier`, so server certificates are checked against
  the operating system's trust store (on Linux the distribution's CA bundle; on macOS/Windows the
  system verifier), like `curl` does today.
- **IMAP**: the spike at the start of phase 1 picks between the `imap` crate (blocking, fits the
  thread model) and `async-imap` (the best maintained one, used by Delta Chat, on a
  single-threaded runtime inside the sync thread). Criterion: maintenance status and `CONDSTORE`,
  `QRESYNC`, `IDLE` and literal size limits.
- **Spike result (2026-11-14)**: neither candidate fits, so Shale ships a small blocking IMAP
  client of its own (`imap_client.rs`, about 600 lines) on `rustls` 0.23 (`ring` provider) and
  `rustls-platform-verifier` 0.7. `imap` is still `3.0.0-alpha.15` (years of alphas, `native-tls`
  by default, no `CONDSTORE`/`QRESYNC` support, no limit on literal sizes). `async-imap` 0.12 is
  maintained, but needs an async runtime (tokio or async-std, a large new dependency for one
  thread), parses whole literals into memory and has no size limit either. `imap-codec`/`imap-next`
  are still alphas. What phase 1 needs is small and fixed (`CAPABILITY`, `LOGIN`/`AUTHENTICATE`,
  `LIST`, `EXAMINE`, `UID SEARCH`, `UID FETCH`), and owning it gives three things no crate offers:
  a literal size limit that skips a too-big message without reading it, the read-only guarantee
  enforced in one place (every command is checked against an allowlist before it is sent, see
  MAIL-023) and an authenticator hook for XOAUTH2 (phase 1c). `IDLE` and `QRESYNC` are not
  implemented (deletions are found with `UID SEARCH ALL`); both can be added later without
  touching the rest. `rusqlite` 0.42 with `bundled` has FTS5 (checked by a test, MAIL-015).
- **Index**: `rusqlite` with the `bundled` feature (SQLite with FTS5 compiled in).
- **Sending** (phase 3): `lettre` (SMTP and building MIME messages).

### 5. Signing and encryption use the system's key stores (phase 4)

Shale does not store private keys. On Linux, GnuPG is the shared key store for both standards:

- **OpenPGP** (`gpg`, PGP/MIME RFC 3156) and **S/MIME** (`gpgsm`, CMS) both keep keys in
  `~/.gnupg`, use the same `gpg-agent` for passphrases (the system pinentry dialog) and support
  smartcards and YubiKeys.
- Shale calls them like `curl` and `secret-tool`: as programs with an argument list, data on
  stdin, and the machine-readable `--status-fd` output for results. (GPGME is the library form
  of the same thing and can replace the programs later.)
- macOS/Windows would use the Keychain / Windows certificate store; out of scope while Shale is
  Linux-first (OPEN-007).

### 6. Security rules

- No HTML rendering, no remote content, no automatic loading of anything (already true).
- Attachments are opened only through the SEC-006 allowlist, from a temp copy.
- The message parser has size limits (header size, MIME depth, decoded size) and fuzz tests;
  a malformed message must never panic the sync job.
- IMAP folder names and search text are quoted by the library, never assembled by hand.
- The local copy is unencrypted, like Thunderbird's. It lives in a 0700 folder; full-disk
  encryption is the protection, and the README must say so.
- Phase 4: decrypted mails **are** stored decrypted and indexed locally (decided below: the device
  is trusted). This is only switched on after a red warning in the encryption settings and typing
  `I agree to decrypted local storage`. Signatures are shown
  from GnuPG's status output only, never from anything in the mail; decrypted parts are never
  mixed with unencrypted parts of the same mail (EFAIL).

## Consequences

- A mail can be read and imported without a second window; Thunderbird keeps working alongside.
- Phase 1 never changes anything on the server, so it can't lose mail.
- New dependencies: rustls, an IMAP crate, rusqlite (C code: SQLite). This weighs against the
  few-dependencies rule, and is the main cost of this ADR.
- 3 years of all folders can be several GB on disk and take a long first sync.
- Tests need a pretend IMAP server (in-process, like the pretend CalDAV server) for `cargo test`,
  plus a local container setup (`podman-compose`, `tests/mail-env/`): Dovecot with test mailboxes
  and a local OAuth2 provider (Keycloak or `mock-oauth2-server`) that Dovecot checks tokens
  against, so the password and the OAuth2 sign-in are tested end to end without a real account.

### 7. OAuth2 (phase 1)

- XOAUTH2 / OAUTHBEARER, authorization-code flow with PKCE, the system browser (through the
  SEC-006 opener) and a redirect to `http://127.0.0.1:<random port>`. Refresh token in the keyring.
- Client IDs are settings with built-in defaults per provider (Google, Microsoft). Shale needs its
  **own** registrations: an Azure app (free, public client) and a Google Cloud project. Google's
  `https://mail.google.com/` scope is "restricted": unverified apps work for up to 100 test users;
  public distribution needs Google's security assessment. Thunderbird's client IDs are not reused.

**Implementation note (phase 1c).** `src/oauth.rs` is the whole flow. The loopback listener
accepts one callback with the right path, `Host` and `state` and answers anything else with an
error page; the token endpoint is called with the system `curl` like CalDAV (form on stdin, no
redirects followed), so no HTTP library was added; SHA-256 and random bytes come from the TLS
library already in the build. The refresh token and an optional client secret are in the keyring,
the access token only in memory in one `TokenSource` per account (shared by the sync's four
connections, so a sync refreshes once). Client IDs are per-account settings, **not** built-in
defaults: until Shale's own registrations exist (the owner's task, above), a person registers an
own app (README). The provider presets are Google and Microsoft plus "Custom" (typed endpoints;
`http://localhost` only here, which is also how the local test environment is used). A refused
token gets one retry with a fresh one; a refresh token that the provider calls `invalid_grant` is
dropped and the person is told to sign in again. The test environment of the Consequences section
is `tests/mail-env/` (Dovecot 2.4 plus `mock-oauth2-server`, run with `podman-compose`; gated tests
with `cargo test -- --ignored mail_env`). Known limits: Google's refresh tokens of an unverified app
in "Testing" status expire after 7 days; Microsoft may insist on `http://localhost` instead of
`127.0.0.1` as the redirect address (not tried against the real service).

## Alternatives considered

- **Read Thunderbird's local copy instead of the server** (extend `tb_store.rs`): no network, no
  password, no sync code. Rejected as the main path because it only works while Thunderbird runs
  and keeps offline copies, and mbox + Mork are fragile to read. It stays as the drag-and-drop
  fallback.
- **Store everything in SQLite** (bodies as blobs): simpler transactions, but one multi-GB file,
  and the mails are no longer plain files. Rejected (plain data, no lock-in).
- **mbox like Thunderbird's default**: see §2.
- **IMAP through `curl`**: one connection per command, no `IDLE`, awkward parsing. Rejected.

## Open questions

- ~~OPEN-015~~ OAuth2: phase 1 (§7).
- ~~OPEN-016~~ Big attachments: downloaded with the message.
- ~~OPEN-017~~ Decrypted mails: stored and indexed locally after explicit typed consent (§6).
- Registering Shale's own Google and Microsoft client IDs is the owner's task; until then a user
  enters their own client ID in the account settings.

## Decisions

- can we multithread these tasks for better performance, since it could be long running tasks? are we safe if the app is closed mid-way syncing? how do we ensure that we are not corrupting any data, or the local sqlite? we should keep a backup of the local sqlite I think, no? it the sqlite gets corrupted we have a restorepoint...?
- the decisions and consequences that you proposed are fine for me.
- OPEN-015: I guess its sensible to support oauth2, yes. phase 1. can we spin up local container env for testing the flow and our implementation?
- OPEN-016: download with the message.
- OPEN-017: I think its reasonable to assume that the local device that runs our application is trusted and therefore its fine to save decrypted mails and also index them. I would say we add an info to the settings of the IMAP encryption stuff, that encrypted mails WILL ALWAYS BE STORED DECRYPTED LOCALLY. we introduce there a big red banner with this info and an input that needs the user to type "I agree to decrypted local storage" to setup the encryption feature in shale.