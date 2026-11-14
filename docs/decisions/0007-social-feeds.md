# ADR-0007: Social feeds — Mastodon and Bluesky, saved as notes

- **Status**: Proposed
- **Date**: 2026-11-14
- **Specs**: SOC-001…SOC-018, SEC-005, JOB-001, JOB-002

## Context

The user wants to scroll a Mastodon or Bluesky feed inside Shale and copy a post into the vault
as a note: with its pictures, a link back, and, for a thread by one author, the whole thread.

Shale has no HTTP library. CalDAV already runs the system `curl` program (SEC-001, SEC-002), and
secrets already go to the system keyring (SEC-003). The app must stay light: no async runtime and
no TLS or OAuth crates.

## Decision

1. **Transport**: the same `curl` helper as CalDAV (`cal_dav::request_bytes`), with a size cap
   and binary bodies. Only `https://` servers (`http://` for this computer). Tokens are sent in
   `curl`'s standard input, never on a command line (SEC-002).
2. **Sign-in, read only**:
   - Mastodon uses OAuth with the out-of-band redirect: Shale registers an app on the server,
     opens the server's page in the browser, and the user pastes the code. The scope is `read`.
     A pasted access token also works.
   - Bluesky uses an **app password** (never the main password) and the session and refresh
     tokens of the AT Protocol. The session lives in memory only.
3. **Secrets**: the access token or app password goes to the keyring (`social_store_or_keep`).
   Only without a keyring does it stay in the owner-only settings file, in the account entry's
   `secret` field. Account entries are written without a secret when the keyring has it. The
   keyring may ask the user to unlock it, so it is only called from background threads.
4. **No login for pictures**: media addresses on other servers (picture CDNs) are fetched without
   any `Authorization` header. The token goes only to the account's own server.
5. **Self-thread rule**: "the whole thread" means the posts of the same author around the
   chosen one: the unbroken run above it, then that author's replies to posts already in the
   thread. Other people's replies are not saved; they would make a note about a conversation, not
   about a post.
6. **Note format**: one note per post or thread, in the front matter `type: social-post`,
   `network`, `author`, `url`, `post-id`, and for threads `thread-ids`. The body has a **Source**
   link back, the text, and `![[z_media/…]]` embeds. A file that is not saved stays a link.
7. **Duplicates**: a note counts as made from a post when its first lines say `post-id: "<id>"`
   or list the id in `thread-ids`. The Import dialog marks such posts, also single posts of a
   thread that was saved before.
8. **Background work**: the window never waits for the network. Feeds, pages, saving and sign-in
   run on threads that send their result over a channel, wrapped in `catch_unwind` (as
   [ADR-0006](0006-engineering-baseline.md) asks). Pictures come from 4 worker threads into a
   cache folder, once per address.
9. **Opening links**: only `http` and `https` addresses from a server are handed to `xdg-open`.

## Consequences

- No new dependencies. A machine without `curl` can't use the feeds (as for CalDAV).
- The Mastodon app registration is created once per sign-in (a few bytes on the server).
- The picture cache grows without limit until pruning exists (SOC-017).
- Video is not played in Shale: the preview shows a play mark and a click opens the browser.
  The note keeps the video file (Bluesky: the real file from the author's server).
- The code is tested against pretend servers only; real servers are tried by hand (SOC-016).

## Alternatives

- **An HTTP crate (`ureq`, `reqwest`)**: more code to vendor and review, and a second way to
  talk to servers next to the CalDAV one.
- **OAuth with a local redirect (or Bluesky's OAuth)**: needs a listening port or a registered
  client. App passwords and the out-of-band code are simpler and can be revoked by the user.
- **Saving the whole conversation**: noisy; the rule above is predictable.
