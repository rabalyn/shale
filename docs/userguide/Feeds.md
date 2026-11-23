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

![[social-thread.png]]

# Like, boost, reply, quote and write

Every post has **Reply**, **Boost** (Bluesky: **Repost**), **Favourite** (Bluesky: **Like**) and
**Quote** buttons, in the feed and in a conversation. **New post** in the toolbar starts a post of
your own.

- **Boost and Favourite / Like** act at once and do not ask. The button is filled when you did
  it; press it again to take it back. A spinner shows inside the button while the server answers
  (the button keeps its size and nothing moves), the count changes when it went well, and a
  message tells you when it did not.
- **A quoted post** inside a card has its own small row of buttons (Reply, Boost, Favourite / Like,
  Quote, Show thread, Save to vault, Open in browser, Copy link). They act on the quoted post, not on
  the card around it. (A quoted Bluesky post does not tell Shale whether you liked it before, so its
  Like button starts unfilled.)
- **The compose box** opens over the tab. It shows how long the post is against the limit of your
  server ("123 / 500"; Mastodon's limit comes from the server, an address counts 23; Bluesky
  allows 300 characters). **Send** or **Ctrl+Enter** is disabled while the post is empty or too
  long. A **reply** starts with the author's @name on Mastodon and uses the same visibility as the
  post it answers if that is not public. Mastodon also has **Content warning** and **who may
  see it** (public, unlisted, followers only, mentioned people only). Bluesky has no content
  warnings for text, so the box has none. Links and @names in a Bluesky post become links.
- **Pictures**: up to four. Press **Choose pictures…** to pick files in the system's file dialog
  (on Linux it is the desktop portal's; if none is available Shale says so and you can still type
  the path), **drop** picture files on the box (it lights up; a file that is no picture is refused
  with a message), or **paste** a picture you copied (Ctrl+V). You can also type the path of a
  picture (a path without a folder at the front starts at your vault) and press **Add picture**.
  Describe each one in its **alt text**
  field. JPEG, PNG, GIF and WebP work. Bluesky takes pictures up to 1 MB, so Shale scales a bigger one down and
  sends it as a JPEG (a too-big GIF is refused); a Mastodon picture over the server's limit is
  refused with its size.
- **Quote**: Bluesky and Mastodon 4.5 or later quote the post for real. On an older Mastodon
  server the box adds the post's web address at the end of your text and tells you.
- **Post this?** Sending always asks first, with your handle and who may see the post: **Post**
  sends, **Cancel** (or Esc) goes back to your text. While it is sent the box is busy and nothing goes out twice. If it
  fails the box stays open with your text and the message.
- When it is out, the box closes and the post waits behind the "N new posts" button of your own
  feed, so the list does not move; a reply also appears in the conversation you wrote it in.

![[social-compose.png]]

**Mastodon needs permission to write.** New sign-ins ask for it. If you signed in before with
read-only access, reading keeps working, and the first like, boost, reply or post says "Sign in
again to allow writing" and opens **Edit** for that account: sign in once more and write.

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
