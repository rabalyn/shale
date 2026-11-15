# Mail test environment

A local Dovecot (IMAP over TLS, seeded mailboxes), its `submission` service (SMTP, for sending) that
relays into a mail sink (Mailpit), and a pretend OAuth2 provider, to try Shale's mail view, OAuth2
sign-in and sending end to end without a real account (MAIL-090…092, MAIL-160). Everything listens
on `127.0.0.1` only. The credentials below are test values; the certificate authority is made
here and is for this environment only.

Needs `podman-compose` (or `docker compose`), `openssl` and `curl`. The images are pinned:
`dovecot/dovecot:2.4.1`, `navikt/mock-oauth2-server:2.3.0`, `axllent/mailpit:v1.27.9`, `alpine:3.24.2` (for seeding).

## Start and stop

```sh
tests/mail-env/mail-env.sh up       # certificates (first time), containers, seeded mail
tests/mail-env/mail-env.sh status
tests/mail-env/mail-env.sh revoke   # restart the provider: every token it handed out is dead
tests/mail-env/mail-env.sh down     # stop (the mail stays in the volume)
tests/mail-env/mail-env.sh reset    # stop and delete the mail; the next `up` seeds it again
```

`SHALE_MAIL_ENV_COMPOSE="docker compose"` selects Docker. The seeded mail is dated relative to the
first `up`, and the certificate lasts a year: after a long time, `rm -r tests/mail-env/certs`,
then `reset` and `up`.

## The two accounts

| | Password account | OAuth2 account |
|---|---|---|
| User name | `alice@shale.test` | `oauth@shale.test` |
| Sign-in | password `shale-test-pw` | browser sign-in at the pretend provider |
| Server | `localhost`, TLS port **10993** (STARTTLS: port **10143**) | the same |
| Mail | INBOX (8, one older than 3 years), Work, Work/Projects, Sent, Drafts, Trash (2), Junk (1) | INBOX (2), Sent, Trash |

What the password account holds, so the sync can be checked by eye: three unseen mails (they must
stay unseen: Shale only uses `EXAMINE` and `BODY.PEEK`), a flagged one, a mail from two years ago, one
from 1200 days ago (outside the 3-year window, not fetched), a 6 MB mail with an attachment, an
HTML-only mail, a mail with ISO-8859-1 (non-UTF-8) headers and body, a nested folder with an old and a
new mail. Trash and Junk are not synced until you open them.

## Sending (SMTP)

Dovecot's `submission` service takes mail from the same two users, signs them in with the same
password or OAuth2 token (`PLAIN`, `XOAUTH2`, `OAUTHBEARER`; only after TLS) and relays what they send to
**Mailpit**, a sink that accepts everything and shows it through an API. Nothing leaves this computer.

| | Port on 127.0.0.1 |
|---|---|
| SMTP over TLS (submissions) | **10465** |
| SMTP with STARTTLS (submission) | **10587** |
| Mailpit's API and web page | **18025** (http://127.0.0.1:18025 shows every mail the tests sent) |

In Shale's account settings, Sending (SMTP): server `localhost`, TLS and port `10465` (or STARTTLS and `10587`),
your name and an address; the sign-in is the one of the account. "Test connection" signs in to
SMTP as well. Send a mail to anyone: it arrives in Mailpit (a Bcc recipient is shown there as `Bcc`, because
the sink sees the envelope), and a copy lands in the user's Sent folder in Dovecot (Shale appends it).
Delete the mails in Mailpit with the page's button or `curl -X DELETE http://127.0.0.1:18025/api/v1/messages`.
The Sent copies of a manual try stay in the seeded Sent folder: `doveadm expunge` can remove them, or `reset`.

## Signed and encrypted mail (phase 4)

`cargo test -- --ignored mail_env` also sends a signed and encrypted OpenPGP mail and a signed and encrypted
S/MIME mail through the submission service into Mailpit (keys are made in a throw-away `GNUPGHOME`, needs
`gpg`, `gpgsm` and `gpgconf` on the host), checks that the message arrived byte for byte, that it verifies and
decrypts for the recipients, and that the copy in Dovecot's Sent folder decrypts for the sender (MAIL-198).

## Trust the certificate

Shale checks server certificates with the operating system's trust store and has no switch to skip
that. The test CA is `tests/mail-env/certs/ca.crt`. Either:

- start Shale with `SSL_CERT_FILE=$PWD/tests/mail-env/certs/ca.crt shale` (or `cargo run`). That
  run trusts **only** this CA (verified: the platform verifier reads `SSL_CERT_FILE`), so other
  HTTPS sites fail in that run; or
- add the CA to the system store for as long as you test (Arch: `sudo trust anchor --store
  tests/mail-env/certs/ca.crt`, Debian/Ubuntu: copy it to `/usr/local/share/ca-certificates/` and run
  `sudo update-ca-certificates`) and remove it afterwards.

## Add the accounts in Shale

Mail view, Accounts…, Add account:

**Password account:** Server `localhost`, Security TLS, Port `10993`, User name `alice@shale.test`,
Sign in with Password, Password `shale-test-pw`. Test connection, Done.

**OAuth2 account:** Server `localhost`, TLS, Port `10993`, User name `oauth@shale.test`, Sign in
with OAuth2, Provider **Custom**:

- Authorization address `http://localhost:18080/default/authorize`
- Token address `http://localhost:18080/default/token`
- Scope `openid mail offline_access`
- Client ID `shale-mail-env` (any), no client secret

Press "Sign in in the browser". The provider's page asks for a user name: type `oauth@shale.test`
(it becomes the token's subject, and Dovecot takes the mailbox from it); leave the claims empty. The
page loads a font from Google Fonts; that is the provider's page, not Shale. Then Test connection.
Plain `http://` addresses are accepted for the Custom provider on this computer only.

Try "revoked": `mail-env.sh revoke`, then sync. The sign-in is refused and Shale says to sign in again.

## The automated test

```sh
tests/mail-env/mail-env.sh up
cargo test -- --ignored mail_env
```

Four tests in `src/mail_env_tests.rs` (ignored by default, so `cargo test` never needs containers).
The fourth (phase 3, MAIL-160) sends from both accounts through the submission service (the password account
over STARTTLS on 10587, the OAuth2 account over TLS on 10465): a mail with a non-ASCII subject and text, a Bcc and
a binary attachment. It reads what arrived from Mailpit's API (the envelope with the Bcc recipient, the
decoded subject, Shale's header block byte for byte with no Bcc in it, the body byte for byte, the attachment intact),
checks that Dovecot's Sent folder got exactly one copy (read, the same bytes, appended once and not again), that a
second run sends nothing, that a wrong password is an authentication error and that the production
certificate check refuses the test CA; it removes its Sent copies and empties Mailpit afterwards.
The third (phase 2, MAIL-113) makes two scratch mailboxes (`Scratch`, `Scratch2`) and puts test mail
into them with `doveadm save`, then marks, flags, moves, deletes (to Trash) and deletes forever through
Shale's own code, checks Dovecot after each step and that the seeded mailboxes are the same at the end;
it removes everything it made, also what a failed earlier run left. The first two:
the password account syncs end to end (folders, the 3-year window, Trash and Junk not synced, the
large mail, HTML as text, ISO-8859-1, flags) and the server's files and flags (read through
`doveadm` and a read-only mount of the volume) are the same before and after, so nothing was
changed on the server; the OAuth2 account signs in through the real login page with PKCE, syncs,
renews its access token, is refused for another user's name, and after `revoke` gets a clear
"sign in again" error. The tests trust the CA as an extra root in test code only. Use
`SHALE_MAIL_ENV_ENGINE=docker` if you use Docker instead of Podman.

## In CI

The `env-tests` job runs this suite together with the CalDAV one when the runner can run containers
(and says `SKIPPED` when it cannot). What it does and what the runner needs:
[tests/caldav-env/README.md](../caldav-env/README.md), "In CI".

## How it is built

- `compose.yaml`: three services. `seed` (Alpine, once) writes the maildirs with exact dates and
  flags (`seed/seed.sh`); `dovecot` serves them; `oauth` is the provider. Dovecot shares the
  provider's network so that it reaches it at `http://localhost:18080`, the very address the browser
  uses: the provider writes that address into its tokens as the issuer, and Dovecot checks it. All
  ports are therefore published on the `oauth` service.
- `mailpit` shares the provider's network too (as `localhost:1025` for Dovecot's relay; its API is
  published as 18025). `mail-env.sh revoke` restarts it together with the provider.
- `dovecot/dovecot.conf` (Dovecot 2.4): TLS required, `PLAIN` for the password user from
  `dovecot/users`, `XOAUTH2` and `OAUTHBEARER` through an `oauth2` passdb that asks the provider's
  userinfo endpoint with the token on every sign-in.
- `setup-certs.sh`: a CA and a server certificate for `localhost` / `127.0.0.1` into `certs/` (git-ignored).

## Limits

The provider is a mock. Its refresh tokens are not remembered across a restart (after `revoke` it
still answers a refresh, with a token for a stranger, which Dovecot refuses); the real
`invalid_grant` answer is tested against Shale's own pretend provider (`src/oauth_fake_tests.rs`).
It is no test of Google's or Microsoft's behaviour: only of the protocol.
