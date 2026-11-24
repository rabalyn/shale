Mails and posts of one conversation, side by side. Back to [[Start here]].

# Thread view

A thread is the mails or posts of one conversation that you imported as notes. Every mail note has a
`thread-id`, the id of the first mail of the conversation (taken from the `References` and
`In-Reply-To` headers when the mail is imported, see [[Import]]). The **Thread** view of the
right sidebar lists the notes with the same `thread-id` as the open note, **oldest first**: one row
per note with the date, the sender and the title; the note you are in is marked with a coloured
background. A click opens the note like any other open (a pinned tab is not replaced).

- Posts saved from the feeds (see [[Feeds]]) work the same way. A post note has `in-reply-to` (the post
  it answers) and a `thread-id` (the first post of the conversation as the network says it, else the
  post it answers, else the post itself). Posts only belong together within one network, because post
  ids are only unique per network. A saved thread of one author is one note and takes part with all
  its posts.
- The view follows the note. A note without other mails in its thread says so; a note without a
  `thread-id` shows a short hint.
- The list does not depend on the `previous` and `next` links. A mail that was imported with **Leave
  the chain** still stands in its place by date.
- Open it from the clock-like icon with three dots in the right menubar, or press **Ctrl+Shift+T**: the
  sidebar opens on the Thread view and the keyboard goes into the list (↑ ↓ move, Enter opens the
  marked note, Esc gives the keyboard back to the note).
- **Ctrl+Shift+PageUp** and **Ctrl+Shift+PageDown** open the previous and the next mail of the thread
  in the order of the list. They do nothing at the ends, in a note without a thread and while a dialog
  is open. On macOS use Cmd instead of Ctrl. You can change all three keys in Settings → Keybindings
  (`global.thread_previous`, `global.thread_next`, `global.thread_overview`).
- The `thread-id` is a property you can edit; the view follows the new value.

#userguide #import #mail #social
