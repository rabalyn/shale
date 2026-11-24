---
features:
  - "Feeds: read Mastodon and Bluesky, save posts to the vault"
---
![[social-feed.png]]

Read your Mastodon and Bluesky timelines next to your notes. The ribbon's speech-bubble button
opens **Feeds** in a **new tab** every time. Back to [[Start here]].

The **Feeds** icon can also be placed in the right sidebar (drag it there or right-click it, see [[Window layout]]): there it
is a compact companion of this tab, the posts of one feed as small rows. A click on a post opens its conversation in a Feeds tab.

# Accounts

**Accounts…** adds a Mastodon account (server, sign-in in the browser, paste the code; or an
access token) or a Bluesky account (handle and an **app password**, never your main password).
Secrets go to the system keyring. **Edit** next to an account signs in to it again (for example with
a new app password after you revoked the old one): network, server and handle stay, a successful
sign-in replaces the saved secret, and the account keeps its feeds, posts and open tabs. A sign-in
for another handle is refused; a failed one keeps the old secret. **Cancel** leaves the form. Try it without an account: `shale --screenshot out.png --view
social` shows made-up posts.

# Reading

- Pick the account and the feed (Mastodon: Home, Notifications, Your posts and your lists; Bluesky:
  Following, Notifications, Your posts and saved feeds). Posts show avatar, name, time, boosts, pictures, quoted posts and link cards. A
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
  description (alt text) below. **◀ ▶** or the Left / Right keys step through the pictures and videos of the
  post; the mouse wheel, **+** / **−** (also the keys `+` `-`) zoom, **100%** / **Fit** (key `0`)
  switch between the picture's own size and the whole picture, and you can drag a zoomed
  picture. **Esc** or **Close** goes back to the feed. A spinner shows while the picture loads, and a
  message says when it cannot be loaded. Pictures behind a content warning open only after
  **Show**.
- **Videos and GIFs play in the viewer:** click the preview of a video or GIF. A spinner shows while the
  file is downloaded (Shale never sends your login to the media server, takes only https and at most
  64 MB), then the player appears and starts: **play / pause** (or **Space**), the bar to jump, mute and
  volume. A GIF plays without sound and again and again; use the speaker to hear it. **◀ ▶** or Left / Right
  go on to the next picture or video and stop the playing one, **Esc** or **Close** closes the viewer and
  stops it, and **Open in browser** opens the video's page. The files are kept with the pictures
  (see [[Data usage and caches]]), so a video seen before plays at once. Videos are played by `ffmpeg`
  (see [[Sounds and videos]]): without it the viewer shows the video's preview picture and says so, and
  **Open in browser** is the way to watch it. A failed download shows a message; the other pictures and
  videos of the post still open.
- **Several feeds at once:** every Feeds tab has its own feed and scroll position. **New tab**
  next to a feed (or Shift+click, middle-click) opens it in another tab. Open Feeds tabs come
  back at the next start ([[Tabs]]).
- **New posts wait, they do not jump in.** Every 5 minutes (change it in Accounts…, 0 = never)
  each open feed looks for newer posts and offers "12 new posts, show" (or press `.`). The list
  never moves until you click. **Check now** looks right away.

![[social-thread.png]]

# Notifications and your own posts

**Notifications** is a feed in the picker of every account. Each notification is a row (what happened,
who did it, how long ago) above the post it is about; the post is a normal card with **Reply**,
**Boost** / **Repost**, **Favourite** / **Like**, **Quote**, **Show thread**, **Save to vault** and the
rest. When several people liked or boosted the same post, one row says "Ada, Bo and 3 others
favourited your post". A follow (or a follow request) shows the person's avatar, name and handle
and **Open in browser** for the profile. Shown are mentions, replies, quotes, favourites / likes,
boosts / reposts, follows, polls that ended, edits of posts you boosted and posts of people you
asked to be told about.

- A **dot** on a row means the server had not counted that notification as read when the list
  loaded. While the list is on screen, Shale tells the server that you have read everything up to
  the newest entry (Mastodon: the notification marker; Bluesky: "seen"), so your other apps
  show it as read too. The dots stay until you load the list again.
- New notifications arrive like new posts: they wait behind **N new notifications — show** (or
  press `.`), the list does not move, and the tab's title counts the unread ones. They are read when
  you show them.
- The posts in your notifications can be found with the search ([[Search syntax]]).

![[social-notifications.png]]

**Your posts** shows what you posted, your replies and your boosts / reposts, newest first, with the
same buttons as any feed. A post you write in Shale waits behind its **N new posts** button, as in
Home.

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
  the card around it. Shale reads the state of a quoted Bluesky post (whether you liked or reposted
  it) along with the page, and before a Like or Repost on a post it does not know yet, so a post you
  already liked is never liked twice.
- **Polls** of Mastodon posts are shown under the text. Before you have voted you see the options with
  radio buttons (check boxes when several may be chosen) and a **Vote** button (it shows a spinner
  while the server answers); afterwards, when the poll is closed, and on your own posts you see a bar
  and a percentage for each option, your own choices ticked, the number of votes and the time left.

![[social-poll.png]]

- **The compose box** opens over the tab. It shows how long the post is against the limit of your
  server ("123 / 500"; Mastodon's limit comes from the server, an address counts 23; Bluesky
  allows 300 characters). **Send** or **Ctrl+Enter** is disabled while the post is empty or too
  long. A **reply** starts with the author's @name on Mastodon and uses the same visibility as the
  post it answers if that is not public. Mastodon also has **Content warning** and **who may
  see it** (public, unlisted, followers only, mentioned people only). Bluesky has no content
  warnings for text, so the box has none. Links and @names in a Bluesky post become links.
- **Pictures** (or one video): up to four. Press **Choose pictures or video…** to pick files in the system's file dialog
  (on Linux it is the desktop portal's; if none is available Shale says so and you can still type
  the path), **drop** files on the box (it lights up; a file that is no picture or video is refused
  with a message), or **paste** a picture you copied (Ctrl+V). You can also type the path of a
  picture (a path without a folder at the front starts at your vault) and press **Add picture**.
  Describe each one in its **alt text**
  field. JPEG, PNG, GIF and WebP work. Bluesky takes pictures up to 1 MB, so Shale scales a bigger one down and
  sends it as a JPEG (a too-big GIF is refused); a Mastodon picture over the server's limit is
  refused with its size.
- **A video**: one per post, never together with pictures or a poll. MP4, QuickTime, WebM and MPEG
  files work, up to what your server allows (Mastodon: its video limit, 40 MB if it names none) and
  never more than 100 MB. Give it a description. Mastodon converts the video first, which can take
  a minute; Bluesky's video service does the same, and Shale waits for it (up to five minutes). Bluesky takes videos up to ten minutes: with ffmpeg installed Shale checks that before it uploads, and sends the picture's size so the video is not drawn with bars. Before
  it uploads, Bluesky is asked whether your account may upload a video now and its answer is shown if
  not. A GIF is added as a picture.
- **Language**: the box has a **Language** list (it starts with **Settings → Feeds → Post language**, which
  starts as the language of your system). It is sent with the post so others can filter by it; choose
  "No language" to send none. The next post of the account starts with the language you chose last (Shale keeps it, also after a restart).
- **A poll** (Mastodon only): **Add poll** gives you two or more options (the server says how many and
  how long they may be), how long the poll stays open, **People may choose several options** and
  **Hide the totals until the poll ends**. A poll and pictures or a video cannot be in one post on
  Mastodon, so the box offers only one of them. Bluesky has no polls.
- **Schedule…** (Mastodon only): pick a date in the month grid (📅; or type it, `2026-11-23`) and set the
  hour and the minute (click a number to type it) in your local time, at least five minutes ahead. The server keeps the post and sends it then; Shale says "Scheduled for
  …" and nothing appears in your feed. **Scheduled posts…** (in the box, and in the feed's ⋯ menu)
  lists what is waiting and lets you **Cancel post**. Bluesky cannot schedule posts.
- **Quote**: Bluesky and Mastodon 4.5 or later quote the post for real. On an older Mastodon
  server the box adds the post's web address at the end of your text and tells you.
- **Post this?** Sending always asks first, with your handle, who may see the post (and the time, for a scheduled one): **Post**
  sends, **Cancel** (or Esc) goes back to your text. While it is sent the box is busy and nothing goes out twice. If it
  fails the box stays open with your text and the message.
- When it is out, the box closes and the post waits behind the "N new posts" button of your own
  feed (Home or Following, and Your posts), so the list does not move; a reply also appears in the conversation you wrote it in.

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
