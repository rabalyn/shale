# ADR-0011: Mail phase 4: OpenPGP and S/MIME (verify, decrypt, sign, encrypt) with the system's GnuPG

- **Status**: Proposed
- **Date**: 2026-11-14
- **Specs**: MAIL-170… (docs/spec/mail.md)
- **Extends**: [ADR-0007](0007-mail-client.md) (§5 and §6, OPEN-017), [ADR-0010](0010-sending-mail.md) §4 (the hook of the sending pipeline), [ADR-0009](0009-mail-server-writes.md) (nothing here writes to the server except through the existing chokepoint), [ADR-0003](0003-data-safety.md), [ADR-0006](0006-engineering-baseline.md)

## Context

Phases 1 to 3 read, organise and send plain mail. Phase 4 adds the two standards people use to sign and
encrypt it: **OpenPGP** (PGP/MIME, RFC 3156) and **S/MIME** (CMS, RFC 8551). ADR-0007 §5 already decided that
Shale does not store private keys and uses the system's GnuPG; the owner's decision on OPEN-017 says that
the device is trusted, decrypted mails *are* stored decrypted and indexed locally, but only after an explicit,
typed consent. The owner's rule from ADR-0003 holds: **we never lose or corrupt data**. A mail client that
handles cryptography has three further ways to do harm, and the design is built around them:

1. **Showing a status that is not true** (a "valid signature" that an attacker typed into the body, a
   signature over only part of the mail shown as if it covered all of it).
2. **Leaking decrypted text** (EFAIL: an attacker wraps an encrypted part in HTML or in other parts so the
   decrypted text is sent out by a client that joins them; or a client decrypting something it was not asked to).
3. **Losing a typed text or a sent mail's readability** (a signing or encrypting job that hangs on a passphrase
   dialog and takes the window with it; an encrypted mail that cannot be read in Sent because it was not
   also encrypted to the sender).

## Decision

### 1. GnuPG as programs, not a library

`gpg` (OpenPGP) and `gpgsm` (S/MIME) are run with `std::process::Command`: an argument list, never a
shell; data on stdin, results on stdout; and **every result is read from the `--status-fd` lines**
(`[GNUPG:] GOODSIG`, `VALIDSIG`, `BADSIG`, `ERRSIG`, `EXPKEYSIG`, `REVKEYSIG`, `TRUST_*`,
`DECRYPTION_OKAY`, `DECRYPTION_FAILED`, `NO_SECKEY`, `SIG_CREATED`, `INV_RECP`, …), never from the
human text and never from anything in the mail. `--status-fd 2` is used (stderr), so no extra file
descriptor has to be passed to the child; only lines that start with `[GNUPG:] ` count, and the user-id
text inside them is unescaped (`\x3a`) and shown as plain text.

- **Why programs and not GPGME, Sequoia (`sequoia-openpgp`) or rPGP.** Shale's point in ADR-0007 §5 is
  that the *user's own key store* is used: the keys in `~/.gnupg`, smartcards and YubiKeys, the shared
  `gpg-agent`, and the system's pinentry. Sequoia and rPGP are OpenPGP implementations with their own key
  store formats (Sequoia's `sq` store, or none): using them means importing the private keys into Shale,
  which ADR-0007 rules out, or talking to `gpg-agent` ourselves (the agent protocol is the hard part, and
  it is what GnuPG already does well). They also add a large dependency tree (hundreds of crates, a
  second crypto stack next to `ring`) to a program that deliberately keeps its dependencies few. GPGME is
  the library form of exactly what Shale does here (it spawns `gpg` itself and parses the same status
  lines), needs a C library and bindings that the build must find, and brings no capability that the status
  interface lacks. **GPGME can replace `gpg.rs` later** (the module is the one place that knows how GnuPG is
  called); nothing else changes.
- **Why `gpgsm` for S/MIME and not NSS or OpenSSL.** S/MIME needs certificate chain validation, trust, CRL and
  OCSP, and private keys. `gpgsm` is the S/MIME front end of the same agent: its keys, the `trustlist.txt`
  of trusted roots, dirmngr for CRL/OCSP, smartcards and pinentry are shared with `gpg`, so the user sets
  up one thing. NSS (Thunderbird's S/MIME store, the profile's `cert9.db` and `key4.db`) and OpenSSL
  (`openssl cms`) would put Shale in the business of managing a second key store and a trust policy of its
  own, or of reading the private keys from a Thunderbird profile; neither is acceptable. Cost: the user
  must import a certificate into `gpgsm` (`gpgsm --import cert.p12`) and trust the root
  (`~/.gnupg/trustlist.txt`); the settings page says how.
- **Thunderbird keeps its OpenPGP keys in its own RNP store** (`~/.thunderbird/<profile>/openpgp.sqlite`
  or the `.thunderbird/…/pubring.gpg`-style files of RNP), not in GnuPG's. Shale does not read it. To use the
  same keys, export them from Thunderbird (Account settings → End-to-End Encryption → *Backup secret key*;
  public keys via the key manager's *Export*) and run `gpg --import file.asc`. The settings page shows
  these steps. S/MIME certificates are exported from Thunderbird's certificate manager as PKCS#12 and
  imported with `gpgsm --import`.
- **Passphrases are never handled by Shale.** `gpg` talks to `gpg-agent`, which starts the system pinentry
  (a graphical dialog, or the smartcard PIN prompt). Because `gpg` blocks while that dialog is open, **every
  GnuPG call runs in a job** (`jobs.rs`), never on the UI thread: the window shows "waiting for the
  passphrase dialog…" with Cancel, which kills the `gpg` process. Each call has a timeout (30 s for lookups,
  5 min for calls that may open a dialog). No `--passphrase`, no `--pinentry-mode loopback` outside the tests.
- `GNUPGHOME` is overridable (tests use a temp home; an advanced user can point Shale at another one with the
  environment variable, which `gpg` itself honours). No automatic key lookup on the network is ever made
  (`--no-auto-key-retrieve --no-auto-key-locate`); the settings page explains `gpg --locate-keys` and WKD as a
  manual step. If `gpg` or `gpgsm` is not installed the feature says so in words and every other part of the
  mail view works as before.

### 2. Reading

`mail_crypto.rs` looks at the raw `.eml` and classifies the **top-level** structure only:

| Structure | What it is | Done with |
|---|---|---|
| `multipart/signed; protocol="application/pgp-signature"` | PGP/MIME signed | `gpg --verify sig -` |
| `multipart/encrypted; protocol="application/pgp-encrypted"` | PGP/MIME encrypted (also signed, when the OpenPGP message is) | `gpg --decrypt` |
| `multipart/signed; protocol="application/pkcs7-signature"` (or `x-pkcs7-signature`) | S/MIME detached signature | `gpgsm --verify` |
| `application/pkcs7-mime; smime-type=enveloped-data` (or no smime-type) | S/MIME encrypted | `gpgsm --decrypt` |
| `application/pkcs7-mime; smime-type=signed-data` | S/MIME opaque signed | `gpgsm --verify --output -` (verifies and extracts the content in one call; never uses a secret key) |
| a text body that is one `-----BEGIN PGP …-----` block | inline OpenPGP, signed (`PGP SIGNED MESSAGE`) or encrypted (`PGP MESSAGE`) | `gpg --verify` / `gpg --decrypt`; read-only and **weaker** (nothing proves what else the mail contains) |

- **The signed bytes are the exact raw bytes of the signed part**, as stored in the `.eml`: from the part's first
  header to the last byte before the CRLF that precedes the boundary (RFC 1847 §2.1), with bare line
  feeds turned into CRLF (a mail that was stored by another program with LF). They go to `gpg` on stdin; the
  signature is written to a private temp file. One changed byte gives `BADSIG`.
- **The status strip is made from status lines only**: valid + the signer's user id / certificate subject +
  fingerprint + trust level (`TRUST_*`); bad; signature made by an unknown key (`ERRSIG` with missing key,
  `NO_PUBKEY`); expired key (`EXPKEYSIG`) / expired signature (`EXPSIG`) / revoked key (`REVKEYSIG`); signed
  only in part (a `multipart/signed` that is not the whole mail); and the signer's address does not match
  `From`. Nothing in the body, the headers or an attachment is ever drawn as a status: the strip is a
  separate widget above the body, in its own colours, and a body text that reads "Good signature from …" stays
  body text.
- **Signed and encrypted, both orders.** Sign-then-encrypt: the decrypted entity is itself analysed
  (a `multipart/signed` inside, or the OpenPGP message was signed in one operation, which `gpg --decrypt`
  reports as `GOODSIG`/`VALIDSIG` too). Encrypt-then-sign: an outer `multipart/signed` whose content is a
  `multipart/encrypted`: the outer signature is verified, then the inner one is decrypted. Analysis recurses
  at most three levels.
- **EFAIL.** Only a *top-level* encrypted entity is ever decrypted. A `multipart/mixed` that contains
  an encrypted part next to other parts, or an inline PGP block with other text around it, is **not**
  decrypted: the strip says so ("This mail mixes encrypted and unencrypted parts and is not decrypted"),
  because decrypted and unencrypted text of one mail would end up in one rendering. A decrypted mail is shown
  **as its own message**: its text and attachments only, never joined with anything from the outer mail except the
  outer headers (From, To, Subject, Date), which the person sees as the mail's headers anyway. The reading
  pane renders plain text only, never HTML and never remote content (already true: ADR-0007 §6); a link
  in the decrypted text is never opened without the person's click, as for any mail.

### 3. Consent, and what is stored after decryption (OPEN-017)

- **Without consent nothing is decrypted and nothing secret is stored.** An encrypted mail shows
  "Encrypted — enable encryption in the settings" and its original `.eml` is unchanged. The encryption
  feature is switched on in **Mail settings → Encryption**, which shows a big red banner
  "**Encrypted mails WILL ALWAYS BE STORED DECRYPTED LOCALLY**" with the explanation (the decrypted copy and its
  search text lie unencrypted in the mail store, protected only by the 0700 folder and by your full-disk
  encryption), and the feature is enabled only when the person types **exactly**
  `I agree to decrypted local storage` (case-sensitive, nothing before or after, not even a space) into
  the field next to it. The consent and its date are stored in `config.json` (`mail_encryption_consent`).
  Withdrawing it switches decryption off at once; **it does not delete** the decrypted copies that exist (they
  are the person's files; deleting them would be a surprise and the opposite of ADR-0003). The settings say so
  in words next to the button, and the copies are still shown and searched.
- **Verification of a signature on a mail that is not encrypted needs no consent**: it reads the mail and
  asks `gpg` for a verdict; nothing is stored. (It needs `gpg`, which may ask nothing: verification never
  needs a secret key, so no dialog opens.)
- **With consent**: the original `.eml` stays untouched. The decrypted entity, wrapped with the outer headers, is
  written next to it as `<uid>.decrypted.eml` (atomically through `fsutil`; a different copy that is already
  there goes to `.trash/` first, nothing is overwritten silently), with `<uid>.decrypted.json` beside it: the
  signature results GnuPG gave when the mail was decrypted (and when), so that reopening the mail does not need
  GnuPG, a passphrase dialog or the key again and still shows what was found then. The **search text of the mail is the
  decrypted text** (index body and attachment names, from the sibling; subject, from, to stay the outer ones).
  The sibling follows the original wherever the original goes: when a message is moved or deleted (the file goes
  to `.trash/`), its sibling goes with it, and a rebuilt index reads the sibling when it exists.
  Decrypting is lazy: it happens when the person opens the mail (a pinentry dialog may appear), not in the sync.

### 4. Writing

- **Per account** (account settings → Encryption): the OpenPGP key (picked from `gpg --list-secret-keys
  --with-colons`) and/or the S/MIME certificate (`gpgsm --list-secret-keys --with-colons`), "sign new mail by
  default", and the preferred standard. Stored in `config.json` as key fingerprints only.
- **In the compose window**: toggles **Sign** and **Encrypt** and the standard (OpenPGP / S/MIME; only the ones
  with a key are offered), and for every address in To, Cc and Bcc a status: *key found* (with trust) or *no
  key*, **looked up in the local keyrings only** (`gpg --list-keys --with-colons <addr>`, `gpgsm
  --list-keys …`; never the network).
- **Encrypt requires a key for every recipient, Bcc and Cc included**; otherwise Send is refused with the
  list of addresses that have none. **The mail is always also encrypted to the sender's own key**, so the
  Sent copy (and the copy in the outbox) can be read later. Encryption to a key that is in the keyring but not
  certified by the user (`TRUST_UNDEFINED`) is allowed and shown as "not certified" (batch `gpg` would refuse;
  Shale passes `--trust-model always` after the user has *chosen* to encrypt to what is in their own keyring,
  as Thunderbird does for accepted keys). Expired, revoked and disabled keys are refused.
- **Bcc.** One message is built for everyone (not one copy per recipient: that would mean several outbox
  entries and several Sent copies for what is one mail to the person). For OpenPGP the Bcc recipients are
  added as *hidden recipients* (`--hidden-recipient`, the packet carries no key id for them), so the others
  cannot see who was on Bcc. S/MIME has no such thing: every recipient's certificate (issuer and serial)
  is listed in the message, so with S/MIME encryption and a Bcc the compose window shows a warning, and
  Send asks once ("Bcc recipients can be told apart by their certificates: send anyway?").
- **Building.** The final bytes come from `compose::finalize(draft, opts, hook)`; the hook is
  `mail_crypto::hook`. It takes the message `lettre` built (headers and body verbatim), cuts the MIME entity
  (the `Content-*` headers and the body) out of it, and puts it into:
  - **PGP/MIME signed** (`multipart/signed`; `micalg=pgp-sha256`; the entity; `application/pgp-signature` with the
    armored detached signature),
  - **PGP/MIME encrypted** (`multipart/encrypted`; `application/pgp-encrypted` "Version: 1"; the armored message;
    sign + encrypt is one `gpg --sign --encrypt`, as Thunderbird does),
  - **S/MIME signed** (`multipart/signed; protocol="application/pkcs7-signature"; micalg=sha-256`; the entity;
    `smime.p7s` base64),
  - **S/MIME enveloped** (`application/pkcs7-mime; smime-type=enveloped-data; name=smime.p7m`, base64; sign +
    encrypt is sign (detached, `multipart/signed`) and then encrypt that entity).

  The bytes that are signed are **exactly the bytes that are sent**: the entity is taken from the built
  message and put into the output unchanged; line ends are CRLF; and the transfer encodings are 7bit (plain
  ASCII short lines without trailing blanks), quoted-printable or base64 (anything else is made QP when signing
  is on), so that no server or relay re-encoding (8bit→QP, trailing space removal, line re-wrapping) can break the
  signature. The sender's own copy in Sent is the same bytes.
- **Protected headers** (an encrypted/signed copy of the Subject and other headers inside the entity, so the
  subject is not visible to the server) are **out of scope** for this phase: the Subject, From, To and Date of
  an encrypted mail are in the clear, which the UI says next to the Encrypt toggle.
- **Never blocking, never losing.** Send runs the hook in a job; the compose window shows "Signing…" and stays
  open and editable-by-nothing until the result is back; on any failure (no key, cancelled passphrase dialog,
  agent not running, a time-out) the window stays as it was and says why; the draft is saved as always; nothing is
  put into the outbox. Only the result (the signed/encrypted bytes) goes into the outbox, where ADR-0010's
  rules apply unchanged (never sent twice, never sent by accident). The outbox holds exactly the bytes sent
  and appended to Sent.

### 5. Tests

A temp `GNUPGHOME` (short path, so the agent's socket path fits) with keys generated by `gpg --quick-gen-key` and
`gpgsm --gen-key`, unprotected (`%no-protection` / empty passphrase through *loopback* pinentry: **tests only**).
S/MIME uses self-signed certificates that are put into `trustlist.txt` as trusted roots, since `gpgsm` cannot
issue certificates; chain building through a CA is therefore not tested, and neither are CRL and OCSP
(the test home sets `disable-crl-checks`). Round trips are checked with the real programs, over the pretend SMTP
server and, gated, over the container environment (Dovecot submission to Mailpit).

## Consequences

- No private key and no passphrase ever passes through Shale, and a Yubikey or smartcard works the way it does
  for every other GnuPG program. The price is a process per operation (tens of milliseconds, plus the user's
  own pinentry time) and the dependency on `gpg`/`gpgsm` being installed (checked and reported).
- The decrypted local copy is a deliberate weakening, accepted by the owner and made explicit by the typed
  consent; its protection is the 0700 folder and full-disk encryption.
- Signatures can be checked and shown without consent, so a signed (not encrypted) mail is verified even by someone who
  never turns encryption on.
- Thunderbird users have to import their OpenPGP keys into GnuPG once; this is a manual step on purpose
  (the keys are the user's).
- Subject lines of encrypted mail are visible to the mail server (no protected headers yet).
- Trust is whatever the user's GnuPG says (web of trust, `trustlist.txt`); Shale has no trust model of its own.

## Alternatives considered

- **Sequoia or rPGP in-process.** Rejected (see §1): not the user's key store, large dependencies.
- **GPGME.** Equivalent to what we do, needs a C library at build time; a later swap is possible.
- **Decrypt in the sync (all mail, up front).** Rejected: it would open pinentry dialogs for thousands of mails in a
  background job, and it stores decrypted text the person has not looked at. Lazy decryption, on opening.
- **Store nothing decrypted (decrypt every time you open the mail).** Rejected by the owner's decision on OPEN-017
  (search must work on encrypted mail).
- **A separate copy per recipient for Bcc.** Rejected (§4): several outbox entries and Sent copies for one mail; hidden
  recipients solve it for OpenPGP and a warning covers S/MIME.
- **Showing decrypted text inside the outer mail's rendering.** Rejected: that is the EFAIL pattern.
- **Verifying inline PGP as if it were PGP/MIME.** Rejected: an inline signature covers only the block, so the strip says
  so and calls it weaker.

## Implementation notes

- `gpgsm` has no `END_ENCRYPTION` status and, for a message it has no key for, only says `DECRYPTION_FAILED`: S/MIME
  encryption counts as done on exit code 0 with output and no `INV_RECP`/`ERROR`, and a failed S/MIME decryption is
  reported as "no certificate of yours fits, or the data is damaged" (`NoKeyOrDamaged`).
- `gpgsm --verify --output -` is how an opaque S/MIME message is read (`gpgsm --decrypt` refuses signed-data).
- The OpenPGP user ids and the S/MIME subject come from `--list-keys --with-colons`; the signer's addresses are compared
  with the From address (`from_ok`), and a mismatch is shown, never silently accepted.
- **Several keys for one address**: the most trusted usable one is taken (the first on a tie); expired, revoked, disabled and
  never-trusted (OpenPGP) keys are never used. `--trust-model always` is passed for OpenPGP encryption to the keys the
  person has in the keyring, because batch `gpg` refuses uncertified keys; the compose window shows "not certified".
- Sign and encrypt in OpenPGP is one `gpg --sign --encrypt` (as Thunderbird does); S/MIME is signed first (detached,
  `multipart/signed`) and then encrypted. Reading handles all of these and encrypt-then-sign.
- A GnuPG call ends when its time is up or the cancel flag is set; the child is killed, and the readers of its output
  report over channels, so a helper process that keeps a pipe open cannot hold the call up.
- The tests that need `gpg`, `gpgsm` and `gpgconf` return at once when they are missing (the CI image has them).
- Not done: protected headers, a server-side key lookup (WKD/keyservers) by Shale itself, S/MIME certificate chain
  building and CRL/OCSP in the tests, key generation or import from inside Shale, GPGME.

## Open risks (kept short)

- Ciphertext shown by `gpg` as `DECRYPTION_OKAY` is trusted for integrity (the MDC check is part of it in current GnuPG);
  a very old GnuPG that allows unprotected messages is not detected beyond that status.
- `gpgsm` without a configured CRL/OCSP service rejects or accepts certificates according to the user's own gpgsm configuration.
