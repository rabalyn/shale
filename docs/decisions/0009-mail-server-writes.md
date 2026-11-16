# ADR-0009: Mail phase 2: writing to the server (flags, move, delete)

- **Status**: Proposed
- **Date**: 2026-11-14
- **Specs**: MAIL-100… (docs/spec/mail.md); amends MAIL-009 and MAIL-023
- **Extends**: [ADR-0007](0007-mail-client.md) (phase 2 row), [ADR-0003](0003-data-safety.md)

## Context

Phase 1 of the mail view only reads: `EXAMINE`, `BODY.PEEK`, and a guard that refuses every other
command (MAIL-023). Phase 2 makes it two-way: mark read / unread, flag, move to another folder,
delete. This is the first time Shale can change or destroy mail that is not a copy. The owner's
rule for ADR-0003 applies in full: **we never lose or corrupt data; losing or deleting because of a
bug is not tolerable.** An IMAP server cannot undo an `EXPUNGE`, and a plain `EXPUNGE` removes
every message that is flagged `\Deleted` in the folder, including ones that another program set.

## Decision

1. **Sync stays read-only.** `sync_account`, `fetch_folder_listing`, `fetch_message` and the guard
   `check_read_only` do not change. They never send a write command. A write happens only as the
   direct result of an action by the person (a button or a key), or of the retry of such an action
   that was queued while the server could not be reached (the queue is an explicit record of what
   the person asked for, see 5).
2. **One chokepoint for writes.** `mail::imap_client::Client::write(cmd: &WriteCmd)` is the only way to
   send a command that changes a mailbox. `WriteCmd` is a typed enum: `Select`, `StoreFlags`
   (add or remove, only `\Seen` and `\Flagged`), `Move`, `Copy`, `MarkDeleted` and `ExpungeUids`.
   The text of the command is built from the enum only (no raw strings), and then passes a second
   gate, `check_write`, that accepts exactly those shapes, with UID sets made of digits, `,` and `:`
   (never `*`). **Plain `EXPUNGE` is never sent**: only `UID EXPUNGE <set>`, which needs
   `UIDPLUS`; without it deletion is refused. A folder is opened with `SELECT` only for a write and
   with `EXAMINE` otherwise, and a write first checks that the folder's `UIDVALIDITY` is still the
   one the local copy knows (otherwise the UIDs mean other mails and the action is refused).
3. **Delete means move to Trash.** The Trash folder is the special-use `\Trash` one (else the usual
   names). `STORE \Deleted` plus `EXPUNGE` is never used outside Trash, except as the fallback of a
   server without `MOVE`: `COPY` to Trash, **verify** the copy (the `COPYUID` answer of `UIDPLUS`,
   else a search for the Message-ID in Trash), and only then `STORE \Deleted` and `UID EXPUNGE` of
   exactly those UIDs. Without `UIDPLUS` that fallback is refused with a clear message. "Delete
   forever" exists only for mail that already is in Trash, behind a confirmation that names the
   count, and expunges only the UIDs the person chose: other messages that carry `\Deleted` (set
   by another program) survive.
4. **Local files are never deleted.** A moved or deleted message's `.eml` stays: when the server
   told the new UID (`COPYUID`) a copy is written to the new folder's path and the index row follows;
   the old file is moved (`fsutil::rename_noreplace`) to `mail/.trash/<date>/<account>/<folder>/…`.
   The index changes in one transaction, after the server confirmed. A crash leaves files and rows
   that the start-up check already repairs.
5. **Offline and failure.** Every action is first written to a `pending_ops` table in the index
   (one row per message and action) together with its optimistic effect, in one transaction.
   A job sends the queue, grouped by folder and action. If the server cannot be reached, the
   rows stay pending (shown as pending in the list) and are retried by timer and by the next
   action. If the server refuses, the optimistic effect is rolled back, the pending row is
   removed and the person is told why. A failed action never changes local state as if it had
   worked.
6. **Conflicts.** Server wins, unless the message has a pending local action. The sync therefore
   does not overwrite the flags of a message that has a pending flags action. Flags are stored with
   `UNCHANGEDSINCE` (CONDSTORE) when the server has it: if another client changed the message
   meanwhile, its flags are read again and the change is applied on top of them. Mail that another
   client moved or deleted meanwhile is found by a `UID SEARCH` before the write: the action ends
   gracefully, the local row is marked `gone_on_server`, and nothing else is touched.
7. **Marking read.** Opening a message marks it `\Seen` on the server only if the setting "Mark as
   read when opened" is on (default on, after a delay of 2 seconds, changeable). Fetching keeps
   using `BODY.PEEK`.

## Consequences

- The "never writes" promise of phase 1 becomes "sync never writes; only the listed user actions
  do". It stays testable: a test records every command of a whole sync against the pretend server,
  and another proves that nothing but the typed enum can produce a write command.
- A `UID MOVE` or `UID COPY` of a big message is done by the server; nothing is downloaded again.
- The pending table makes the app safe against a closed lid or a dead network, at the cost of a
  second place that holds state; it is a cache like the rest of the index, but a lost queue loses
  only the person's unsent intention, never mail.
- The local trash grows by the moved messages' files. They are copies of mail that exists on the
  server; pruning stays the person's decision (as in phase 1).
- Servers without `UIDPLUS` can still mark and flag, but not delete forever and not fall back from
  a missing `MOVE`.

## Implementation notes

- `WriteCmd` also has `Select`, `Copy` and `MarkDeleted` (set or clear `\Deleted`); the "copy, verify,
  mark, expunge" fallback is a sequence in `mail/ops.rs` made of these, not one variant, because it must
  look at the server between the steps. `Client::write` refuses a write before any `Select`,
  `UID EXPUNGE` without `UIDPLUS` and `UID MOVE` without `MOVE` before anything is sent.
- The queue is the table `pending_ops` (index schema version 2): one row per message and action, with
  the flags before the optimistic change so a refusal can undo exactly that change (and not another
  action's). A message with a waiting move, delete or delete-forever takes no further action; flag
  actions can stack and are sent in order.
- A write is preceded by `UID SEARCH UID <set>` in the folder just opened: UIDs that are no longer there
  are treated as moved or deleted by someone else. After a `MOVE`, `COPY` or `STORE` the result is read
  back (`UID SEARCH`, `UID FETCH FLAGS`) and the index follows what the server says, not what was asked.
- Moves and deletes put the message's row into the target folder only when the server said the new UID
  (`COPYUID`, or a Message-ID search in the fallback) and the old row's file could be read; otherwise
  the next sync or listing of the target brings it. The old file always goes to `.trash/<date>/`.
- After 12 failed attempts to reach the server an action is given up and rolled back, with a message.
- Not tried: a server that offers `MOVE` but not `UIDPLUS` is covered by the pretend server only; the
  container's Dovecot has both (`MOVE`, `UIDPLUS`, `CONDSTORE`), so the fallback path is tested with the
  pretend server alone.

## Alternatives considered

- **`STORE \Deleted` and `EXPUNGE` for delete**: rejected, it can remove other mail (see Context).
- **Send flag changes during sync**: rejected, it would make "sync never writes" untestable.
- **Block the window until the server answered**: rejected; a queue with optimistic display is
  what mail clients do, and the failure path (rollback with a message) is tested.
