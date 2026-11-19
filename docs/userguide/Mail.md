---
features:
  - "Mail"
---
![[mail-view.png]]

Open the **Mail** button in the ribbon (or Ctrl+Shift+M): accounts and folders with unread
counts, the message list (newest first) and the message as plain text. Shale reads your mail
over IMAP and keeps a local copy next to the other program data (`~/.local/share/shale/mail`),
so it works offline and search is instant. Back to [[Start here]].

## Safe by design

- **Syncing only reads.** Fetching never changes the server; only your own actions are written
  back: read or unread, flag, move, delete (always to Trash) and, inside Trash, delete forever.
  A change shows at once and is sent in the background; without a network it waits.
- **No HTML, no tracking.** Nothing is rendered, no pictures are fetched, no scripts run.
- **The local copy is not encrypted** (like Thunderbird's): use full-disk encryption.

## Accounts

![[mail-accounts.png]]

**Accounts…** adds an account: server, port (993 with TLS, 143 with STARTTLS), user name and
password. The password goes into the system keyring, never into the settings file. **Test
connection** checks the login. Sign in with **OAuth2** (Google, Microsoft or your own provider)
with your own client ID; the provider setup steps are in the README. Folders to sync are chosen
with tick boxes; Trash, Junk and "All Mail" load on demand. New mail is fetched every 10
minutes (0 = off). "Mark as read when opened" is on, after 2 seconds.

## Changes made in another program

While the mail view is open, Shale keeps the folder you have open and the Inbox of each account
fresh. It asks the server to say when something changes (IMAP IDLE), so a flag you remove, a
mail you read or a new mail in Thunderbird or on your phone shows here within a few seconds,
without pressing Sync. This connection only reads. A server without IDLE is asked once a
minute instead. When you leave the mail view (or open another folder), the connection of the
folder you left is closed. The regular sync every 10 minutes still looks at all folders.

## Reading and keys

Search with `/` (words, `"phrases"`, `from:`, `to:`, `subject:`, `body:`). One of the three
panes has the keyboard and is framed; Ctrl+H / Ctrl+L (macOS: Cmd+Shift+H / Cmd+Shift+L) move it. j / k, `u` read or unread, `s`
or `f` flag, `d` delete, `m` move, `i` import to the vault ([[Import]]). All mail keys are in
[[Keyboard shortcuts]] and can be changed in Settings → Keybindings.

After a restart a Mail tab comes back on the folder and the message you had open (the first folder if
that folder or account is gone), without asking for anything. See [[Tabs]].

## Writing and sending

- `c` compose, `r` reply, `Shift+R` reply all, `Shift+F` forward. Plain text, with attachments
  (a file, a file of the vault or a note). To, Cc and Bcc complete from your people notes
  ([[Tags and people]]). **Ctrl+Enter** sends.
- Set up sending in the account settings ("Sending (SMTP)": TLS 465 or STARTTLS 587, your name,
  address and signature). The wait before sending ("Undo send", 10 seconds) is set there too.
- **Never lost, never twice.** Drafts are saved as you type. **Send** puts the mail into the
  Outbox with an **Undo send** bar. If the server refuses it stays there and you choose Retry,
  Edit or Discard; nothing is retried by itself.

![[outbox-failed.png]]

If the connection ends after the whole mail was handed over, nobody can know whether it went
out: Shale never sends it again by itself.

![[outbox-unsure.png]]

- Questions before sending: no subject, no text, or "attached" but nothing attached.

## Signed and encrypted mail

Shale uses the **GnuPG on your computer** (`gpg` for OpenPGP, `gpgsm` for S/MIME) with your own
keys and passphrase dialog; it never sees a key or passphrase. A strip above a mail says what
GnuPG found (valid, bad, unknown signer, expired). Writing: Sign and Encrypt in a new mail, the
key of an account in Accounts…, "Signing and encrypting".

**Read this first:** with encryption switched on, **encrypted mails will always be stored decrypted locally**, next
to the encrypted original. It is on only after you type `I agree to decrypted local storage`
in Accounts…, Encryption. Without it nothing is decrypted.

The subject and addresses of an encrypted mail are not encrypted. Without GnuPG installed Shale
says so and everything else works.

### Keep the key unlocked

GnuPG's agent forgets a passphrase after 10 minutes without use and after 2 hours at the latest, and
always when it is restarted, so a signing key has to be unlocked again and again. Shale never handles
the passphrase; the times are GnuPG's own setting (`gpg-agent.conf`), not a Shale setting. Accounts…,
Encryption, "Keeping the key unlocked" shows the current times in words. Two ways to make them longer:

- **With Shale.** Choose "Ask for the passphrase again after" (8 hours, 1 day or 1 week) and click
  "Change with gpgconf…". Shale shows the file, the two lines and what GnuPG's `gpgconf` does with them
  (a marked block; older lines for these two options are commented out; nothing else changes) and writes
  nothing yet. "Write with gpgconf" writes and reloads the agent; Cancel writes nothing.
- **By hand**, if you do not want an app to edit your configs: put `default-cache-ttl N` and
  `max-cache-ttl N` (in seconds) into `gpg-agent.conf` in GnuPG's home folder (the section shows the
  lines for your choice, with a Copy button) and run `gpgconf --reload gpg-agent`.

To keep the passphrase across restarts, let the passphrase dialog save it in the system keyring: on Linux
tick "Save in password manager" (Qt and GNOME pinentry; KWallet or GNOME Keyring), on macOS "Save in
Keychain" (pinentry-mac). Gpg4win's dialog on Windows has no such option: use the longer times. If
`no-allow-external-cache` is set in `gpg-agent.conf`, the section says so: the dialog then cannot save to
the keyring.

#userguide #mail
