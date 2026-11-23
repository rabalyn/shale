---
features:
  - "Feeds: read Mastodon and Bluesky, save posts to the vault"
---
![[social-feed.png]]

Read your Mastodon and Bluesky timelines next to your notes. The ribbon's speech-bubble button
opens **Feeds** in a **new tab** every time. Back to [[Start here]].

# Accounts

**Accounts…** adds a Mastodon account (server, sign-in in the browser, paste the code; or an
access token) or a Bluesky account (handle and an **app password**, never your main password).
Secrets go to the system keyring. **Edit** next to an account signs in to it again (for example with
a new app password after you revoked the old one): network, server and handle stay, a successful
sign-in replaces the saved secret, and the account keeps its feeds, posts and open tabs. A sign-in
for another handle is refused; a failed one keeps the old secret. **Cancel** leaves the form. Try it without an account: `shale --screenshot out.png --view
social` shows made-up posts.

# Reading

- Pick the account and the feed (Mastodon: Home and your lists; Bluesky: Following and saved
  feeds). Posts show avatar, name, time, boosts, pictures, quoted posts and link cards. A
  content warning stays closed until you click Show.
- **Replies and the thread of a post:** click the number of replies under a post, or **Show thread**
  (offered for a post that has replies or is a reply). The conversation opens in the same tab: what
  the post replies to is above it, the post itself is highlighted, and the replies are below as a
  tree, moved in a little more for each step down (after six steps they stay at the same
  indent). The replies are cards like in the feed: pictures open big, links work, content
  warnings hide, and **Save to vault**, **Open in browser** and **Copy link** are there. A spinner
  shows while the conversation loads; if it fails you get the message and **Retry**. **← Back**
  (or the tab's Back button) returns to the feed exactly where you were; clicking the replies of
  another post in the conversation opens that one as the next step. **Shift+click** or a middle
  click opens the conversation in a new Feeds tab. When the server did not send all replies a note
  says so, with **Open in browser** to read the rest on the server's page.
- **Pictures big:** click a picture to see it large over the tab, fitted to the window, with its
  description (alt text) below. **◀ ▶** or the Left / Right keys step through the pictures of the
  post; the mouse wheel, **+** / **−** (also the keys `+` `-`) zoom, **100%** / **Fit** (key `0`)
  switch between the picture's own size and the whole picture, and you can drag a zoomed
  picture. **Esc** or **Close** goes back to the feed. A spinner shows while the picture loads, and a
  message says when it cannot be loaded. Pictures behind a content warning open only after
  **Show**. A video or GIF preview still opens in the browser.
- **Several feeds at once:** every Feeds tab has its own feed and scroll position. **New tab**
  next to a feed (or Shift+click, middle-click) opens it in another tab. Open Feeds tabs come
  back at the next start ([[Tabs]]).
- **New posts wait, they do not jump in.** Every 5 minutes (change it in Accounts…, 0 = never)
  each open feed looks for newer posts and offers "12 new posts, show" (or press `.`). The list
  never moves until you click. **Check now** looks right away.

# Search the posts you loaded

Every post a feed brings in is kept in a local search index. The sidebar search (Ctrl+Shift+F)
lists matching posts under the notes and mail, and the keyboard reaches them ([[Search and pickers]]); `from:ada`, `text:`, `in:`,
`link:` and `date:2026-11` limit a word to one part (all of it in [[Search syntax]]). Settings → Feeds → Post search index
shows how many posts are indexed, **Rebuild**s or **Clear…**s it.

# Save to vault

**Save to vault** copies the post, and the whole thread when it is by one author, into the
Import dialog ([[Import]]): choose the folder (default `Social`), tags and which pictures to
bring along.

![[social-import.png]]

"Include replies from others" adds a Replies section (off by default). The note has the author,
the date and the link back to the post as properties, then the text and the pictures saved in `z_media/`.
Tick **Header list in the note body** in the Import dialog to also get them as a list (Author, Posted,
Source, …) below the title; see [[Import]].

#userguide #feeds
