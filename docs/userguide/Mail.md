---
features:
  - "Mail"
---
# Mail

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

## Reading and keys

Search with `/` (words, `"phrases"`, `from:`, `to:`, `subject:`, `body:`). One of the three
panes has the keyboard and is framed; Ctrl+H / Ctrl+L move it. j / k, `u` read or unread, `s`
or `f` flag, `d` delete, `m` move, `i` import to the vault ([[Import]]). All mail keys are in
[[Keyboard shortcuts]] and can be changed in Settings → Keybindings.

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

#userguide #mail
