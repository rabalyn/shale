# ADR-0010: Mail phase 3: writing and sending mail (SMTP)

- **Status**: Proposed
- **Date**: 2026-11-14
- **Specs**: MAIL-120… (docs/spec/mail.md)
- **Extends**: [ADR-0007](0007-mail-client.md) (phase 3 row, sections 4 and 6), [ADR-0009](0009-mail-server-writes.md) (the write chokepoint gets `APPEND`), [ADR-0003](0003-data-safety.md)

## Context

Phases 1 and 2 read and organise mail. Phase 3 lets a person write, reply, forward and send. The
owner's rule for ADR-0003 applies in full: **we never lose or corrupt data**. Sending mail adds two
failure modes that reading never had, and both are worse than a crash:

1. **A text the person wrote is lost** (a crash, a closed window, a failed send).
2. **A mail is sent that the person did not send, or is sent twice.** A sent mail cannot be
   called back, and SMTP gives no way to ask "did you get that?": if the connection breaks after
   the last byte of the message was written and before the server's answer, nobody can know.

Also: after a send the message should appear in the Sent folder, on the server (so every other
program sees it) and locally at once, without making a second mail on the server.

## Decision

### 1. Libraries: `lettre` builds the message, Shale speaks SMTP itself

- **`lettre` 0.11 with only the `builder` feature** builds the MIME message (header encoding of
  non-ASCII names and subjects, address parsing and IDNA, multipart/mixed with attachments,
  transfer encodings). It adds six small crates and no network code, no TLS, no `native-tls`/OpenSSL.
- **The SMTP conversation is Shale's own small blocking client** (`mail/smtp_client.rs`, the same shape as
  `mail/imap_client.rs`), on the same `rustls` (ring) and `rustls-platform-verifier` set-up as IMAP
  (`mail::imap_client::platform_tls_config`, one place that decides how certificates are checked).
  `lettre` *can* use the platform verifier (feature `rustls-platform-verifier`), so that is not the
  reason. The reasons are safety:
  - **The phase of a failure is known.** "Never send twice" needs to tell "the server cannot have
    the message" (connect, TLS, sign-in, `MAIL FROM`, `RCPT TO` refused, the connection broke
    while the body was still being written) from "the message may have been delivered" (the end of
    the body was written and no answer came). `lettre`'s transport does not say which step
    failed; its errors are kinds, not phases.
  - **OAUTHBEARER.** `lettre` signs in with PLAIN, LOGIN and XOAUTH2 only. The same token source as
    IMAP ([ADR-0007](0007-mail-client.md) §7) also offers OAUTHBEARER, and a server that offers
    both gets the better one, like with IMAP.
  - One TLS code path, no second connection pool, no `socket2`/`url`/`base64` dependencies of the
    transport feature.

  The cost: about 400 lines of protocol code that a library would otherwise own, covered by a
  pretend server and the container test. Only `EHLO`, `STARTTLS`, `AUTH`, `MAIL FROM`, `RCPT TO`,
  `DATA`, `RSET` and `QUIT` are ever sent; the command text is built from typed values.

### 2. Account settings

Per mail account: SMTP host, port, security (**implicit TLS on 465** or **STARTTLS on 587, mandatory**:
nothing, not even `AUTH`, is sent before the connection is encrypted; there is no plain-text choice),
sign-in = the same as IMAP (the same password in the keyring, or the same `TokenSource`), From name
and address, an optional signature, and "the server stores sent mail itself" (default on for the
Google preset, which saves every sent mail in Sent). Provider presets fill the server:
Google `smtp.gmail.com:465`, Microsoft `smtp.office365.com:587`. Microsoft's OAuth2 scope gets
`SMTP.Send` added. "Test connection" also connects to SMTP and goes as far as `AUTH`; it never sends.

### 3. Composing

Plain text only: no HTML authoring, no `format=flowed`. To, Cc, Bcc with completion from the e-mail
addresses in the people notes and from the addresses seen in the mail index, Subject, body (a
multi-line text field; Shale's note editor is bound to notes and a vault, so it is not reused),
attachments (file picker, a file from the vault including `z_media`, or "Attach note": the note's
markdown as `text/markdown`). Reply, Reply-all and Forward quote the original inline
(`On <date>, <name> wrote:` and `> ` per line; a forward keeps the original's attachments) and set
`In-Reply-To` and `References` correctly. Everything a person types into a header field is
validated (no CR or LF, no NUL; addresses must parse); encoding is `lettre`'s job.

### 4. A final-message pipeline for phase 4

`compose::finalize(draft) → build → hook → bytes`. The hook is a function from the built message to the
message to send; today it is the identity. Phase 4 puts OpenPGP and S/MIME there (sign and/or encrypt
the MIME entity) without touching drafts, outbox or SMTP: what goes into the outbox and what is
appended to Sent is exactly the output of the pipeline.

### 5. Drafts: a text is never lost

- A compose window autosaves its draft after a short pause in typing (about 1.5 s) and when it is
  closed: `mail/drafts/<account>/<id>.eml` (the message as it would be sent, for any other program) and
  `<id>.json` (the fields as typed, the reply it belongs to). Attachments are copied into
  `<id>.att/` when they are added, so a draft survives the original file being moved. All writes go through
  `fsutil` (temp, fsync, rename). After a crash the drafts are listed and the person is told.
- Closing a compose window with changes never discards silently: it asks **Keep draft** or
  **Discard**. Discard moves the draft's files to `mail/.trash/<date>/drafts/…`. A compose window
  with nothing the person typed (a pristine reply, an empty new mail) closes without asking.
- A server-side Drafts folder (IMAP `APPEND`) is **not** done in this phase. It would have to go
  through the same chokepoint as the Sent copy and needs a replace-by-Message-ID step; local drafts
  cannot be lost by a flaky connection, so they are the safer first step.

### 6. The outbox: never twice, never by accident

Pressing Send writes the message into the outbox and nothing else:

```
mail/outbox/<account>/<id>.eml     exactly the bytes that will be sent (and appended to Sent)
mail/outbox/<account>/<id>.json    state, error, envelope, send-after time, the draft fields, flags
mail/outbox/<account>/<id>.att/    the attachments of the draft (for editing again)
```

State machine, every transition one atomic write: `queued → sending → sent`, `sending → failed`
(the server refused, or the failure was before the end of the body), `sending → unsure` (the end of the
body was written and the answer never came), `unsure → queued` (the person's explicit **Send again**),
`failed → queued` (**Retry**), `queued → undone` (**Undo**).

- The transition to `sending` is written **before** the first byte goes to the network, under a lock, and only
  from `queued`. A message found in `sending` at start-up (the program died in the middle) becomes
  `unsure` and is **never resent automatically**: "may have been sent, check Sent, then Send again or
  Discard".
- **Undo send**: a message stays `queued` for N seconds (setting, default 10, 0 = at once). Undo before
  the transition to `sending` pulls it back into an editor (the order is: mark `undone`, write the draft,
  move the outbox files to `.trash`, so a crash in between can never send it).
- A failure keeps the message in the outbox with the server's words; only the person retries. Nothing is
  retried by a timer, because a retry after an ambiguous failure is exactly the double send.
- Before Send, a confirmation is asked for an empty subject, an empty body, and a text that mentions an
  attachment (`attached`, `attachment`, `angehängt`, `anbei`, `Anhang`) with none attached.
- `Bcc` is used for the envelope only and is never in the headers of the message.
- The outbox files of a sent message stay (state `sent`); removing them is the person's action (to
  `.trash`). Nothing deletes a file.

### 7. The Sent copy

After the server accepted the mail:

1. The outbox JSON becomes `sent` (atomic).
2. The exact sent bytes are stored locally as a message of the account's Sent folder (so it shows in
   the list at once) **and** an `append` action is recorded in `pending_ops`, in one transaction. The local row
   gets a synthetic UID in a range no server uses and the sync leaves it alone while the action is pending.
3. The action is run like the phase 2 ones: `WriteCmd::Append { folder, flags: [\Seen], bytes }` through the
   write chokepoint of `imap_client` (ADR-0009): typed, `check_write` accepts exactly
   `APPEND "<folder>" (\Seen) {<n>}` with `n` at most 100 MiB and the bytes are exactly `n` long, no NUL.
   Before an append is retried, the folder is searched for the Message-ID: a mail that is there is not appended again.
   The new UID (`APPENDUID`, else the Message-ID search) replaces the synthetic one.
4. The Sent folder is the special-use `\Sent` one, else the usual names (`Sent`, `Sent Items`, `Sent Messages`, `Gesendet`).
   Accounts that store sent mail themselves skip 2 and 3 (the sync brings the server's copy).

An append that fails after a successful send **never causes a resend**: the mail stays sent; the append
waits in `pending_ops` and is retried like any queued action; a refusal is shown ("sent, but the copy to
Sent failed: …").

### 8. Message-ID, Date, no tracking

`Message-ID: <random@sender-domain>` (96 random bits, the domain of the From address), `Date`, `MIME-Version`.
`User-Agent` is a setting, off by default. No read receipt is ever requested.

### 9. Keys

In the mail view, with no text field in use: `c` compose, `r` reply, `Shift+R` reply to all, `Shift+F` forward
(`f` stays the flag key of phase 2: the phase 2 keys accept no modifier, so `F` is free). In a compose
window: `Ctrl+Enter` send, `Esc` close (with the draft rule).

## Consequences

- Sending is safe against crashes at every step and the failure that cannot be resolved (an unanswered
  `DATA`) is shown to the person instead of guessed. It costs one more state ("may have been sent") the
  person has to decide on, and there is no automatic retry of a failed send at all.
- The outbox, drafts and Sent copies are plain files next to the mail store, with the same backup and
  `.trash` rules (nothing is deleted by code).
- A hand-written SMTP client is ours to maintain. It speaks only what sending needs and is tested against a
  pretend server in several modes and, in the container environment, against Dovecot's `submission`
  service relaying into a sink (Mailpit).
- OAUTHBEARER over SMTP is supported by us only; XOAUTH2 (Gmail, Outlook) works with every server that supports OAuth.
- No server Drafts folder: a draft written in Shale is not visible in other programs.

## Implementation notes

- `lettre` is used with `default-features = false, features = ["builder"]`: six small crates in total (`email-encoding`,
  `email_address`, `httpdate`, `mime`, `quoted_printable`, `lettre`). A message that Shale builds is always CRLF, ASCII headers and
  7-bit or encoded bodies; `smtp_client` refuses a message with a bare CR or LF (SMTP smuggling) instead of repairing it.
- The command gate of the SMTP client is `check_command`; a long `AUTH` line is allowed (OAuth2 tokens are several hundred
  bytes, found by the container test), every other line is at most 600 bytes. If any recipient is refused nothing is sent to the
  others (the transaction is reset): no mail goes to only some of the people.
- The compose window uses a plain multi-line text field, not the note editor (which is bound to notes and a vault), and has no
  vim-style editing. Attachments are added by path, from a list of the vault's files, or as a note; dropping files onto the
  window is not done.
- The local Sent copy is kept under UIDVALIDITY 0 (`mail::db::LOCAL_UIDVALIDITY`, which RFC 3501 forbids a server to use) with a UID of
  4,000,000,000 and up (`SYNTHETIC_UID_BASE`) and its own file `<account>/<folder>/0/<uid>.eml`, until the server's UID is known. The index key is
  (folder, UIDVALIDITY, UID), so a real server message can never overwrite or merge with it (changed in ADR-0011 step 0; before, it shared the
  server's UIDVALIDITY and relied on the range). The folder list shows rows of UIDVALIDITY 0 next to the current ones. The next sync replaces a
  local copy whose Message-ID the server has (and for which no append waits) by the server's row; the local file moves to `.trash/`.
  A flag action on it is refused while the append waits. If the server did not say `APPENDUID` the UID is found by Message-ID; if that fails
  the local copy goes to `.trash/` and the next sync brings the server's own.
- Drafts are autosaved on the window's thread (small JSON and the `.eml`; attachments are capped at 25 MB together).
- The outbox lock is per process: two Shale programs on the same mail folder are not supported.
- Not done in this phase: a server-side Drafts folder, HTML mail, `format=flowed`, dropping files into the compose window,
  a separate window per draft (one compose view at a time; others wait in Drafts).

## Alternatives considered

- **`lettre`'s `SmtpTransport`**: less code, but the phase of a failure is lost (see 1) and no OAUTHBEARER. Rejected for the safety reason.
- **Send first, store after**: simpler, but a crash after the send loses the record that it was sent. Rejected: the outbox is written first.
- **Automatic retry of a failed send**: friendly, and exactly the double send. Rejected.
- **HTML mail, format=flowed**: out of scope; plain text only.
- **Drafts on the server**: see 5.
- **`mail-send` or `mailparse`-style crates**: another dependency set for what `lettre`'s builder already does.
