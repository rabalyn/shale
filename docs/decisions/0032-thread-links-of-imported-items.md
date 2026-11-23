# ADR-0032: Thread links of imported mails and posts

- **Status**: Accepted (owner decision 2026-11-23)
- **Date**: 2026-11-23
- **Specs**: IMP-067…IMP-073 ([`docs/spec/features.md`](../spec/features.md)), SOC-070…SOC-072 ([`docs/spec/social.md`](../spec/social.md)), NAV-070, KEY-050 ([`docs/spec/links-and-editor.md`](../spec/links-and-editor.md)); builds on IMP-003, SOC-011, VAULT-041…043
- **Issue**: rabalyn/shale#575
- **Builds on**: [ADR-0004](0004-link-resolution-and-paths.md) (links), [ADR-0014](0014-sidebar-layout.md) (right-sidebar views), [ADR-0028](0028-file-history.md) (history snapshots)

## Context

An imported mail (IMP-003) or post (SOC-011) is one note. A follow-up mail, or a reply to a post
already in the vault, becomes a second, unrelated note: nothing tells the person that the two belong
together, the import dialog puts it in the folder used last, and the thread has to be rebuilt by hand.
#543 (`import::mail_segments`) already cuts the quoted mails out of a mail; this record is about the
**links between the notes** of one thread.

What is on the notes today: a mail note has `message-id` (with the angle brackets, `"<abc@host>"`); a
post note has `post-id` (the network's id) and, for a saved self-thread of one author, `thread: N` and
`thread-ids: [...]` (all posts inside that one note). Neither has the id of the parent or of the root.

The owner decided on 2026-11-23: link format **C** (below), matching by **reply headers only**, a
suggestion and not an automatic action, the new note placed **next to** the earlier one, a question
when an **older** item arrives later, and a **thread overview** with keys.

## Decisions

1. **Properties** (front matter, written by the import, one header per note, IMP-058). All names are
   lower case; a property is left out when it has no value.

   | Property | Mail | Post | Value |
   |---|---|---|---|
   | `message-id` | yes (exists) | | text, with the angle brackets: `"<abc@host>"` |
   | `post-id` | | yes (exists) | text, the network's id of the post |
   | `in-reply-to` | yes | yes | mail: the `In-Reply-To` Message-ID, `"<x@host>"`; post: the `post-id` of the post it replies to |
   | `references` | yes | | list of the `References` Message-IDs in header order, `["<r1@h>", "<r2@h>"]` |
   | `thread-id` | yes | yes | text: the id of the root (see 2) |
   | `previous` | yes | yes | one wikilink in quotes: `"[[2026-11-20 Plan]]"` |
   | `next` | yes | yes | one wikilink in quotes: `"[[2026-11-24 Re Plan]]"` |

   `thread-id` is new and is not `thread` / `thread-ids` of a saved self-thread of one author
   (SOC-011), which stay as they are: those list the posts **inside** one note, `thread-id` ties
   **several notes** together. A post match also needs the same `network` (already a property): post
   ids are only unique per network.
2. **The root.** Mail: the first Message-ID of `References`, else `In-Reply-To`, else the mail's own
   `message-id` (a mail that starts a thread is its own root). Post: the root of the conversation as
   the network says it (Mastodon: the first status of the conversation context; Bluesky: the root of
   the reply reference), else the post's own `post-id`. A note is written with `thread-id` as soon as
   its headers name a root, **also when no other note of the thread is in the vault yet**, so a later
   import finds it. A mail that quotes nothing and replies to nothing, and has no reply yet, gets its
   own id as `thread-id` and no `previous` or `next`.
3. **Matching** uses the reply headers only, never the subject, the sender or the text. For an item
   being imported the candidates are the notes of the vault that
   - have a `message-id` / `post-id` equal to the item's `in-reply-to` or one of its `references`
     (the item is a follow-up), or
   - have an `in-reply-to` or `references` naming the item's id (the item is **older** than a note
     already in the vault), or
   - have the same `thread-id` (and, for a post, the same `network`).

   A saved self-thread note matches by every id in its `thread-ids` too. Ids compare without the
   angle brackets (`import::bare_id`, as the Thunderbird import does). The result is **offered, not
   applied**: the Import dialog shows a line "Continues `[[note]]`" with a tick box (ticked) and, when
   more than one note qualifies, a choice (nearest by headers first: the note whose id is the item's
   `in-reply-to`; then by date). The person can untick it; then the note is written with its own ids
   and `thread-id` but with no `previous` / `next`, and no other note is touched.
4. **Placement.** With a match, the **folder** of the import is preset to the folder of the matched
   note (the one chosen in the choice), when the person has not chosen another one. The IMP-002 rule
   that the drop target decides the folder is not overridden: a folder that was dropped on, or a path
   edited by hand, wins. The note's name is not changed (IMP-003).
5. **Order and reordering.** `previous` / `next` follow the **date** of the items (the `Date` header,
   the post's time), oldest first, one chain per `thread-id`, also when the thread branches (a tree is
   shown as a line, by time). Ties break by the id text, so the order is the same on every device. A
   new item that is **newer than all** others is appended: it gets `previous`, and the last note gets
   its `next` (one other note is rewritten). An item that is **older than some note** in the vault
   would sit in the middle: the dialog asks "Keep the thread in date order? This changes the
   `previous` and `next` of N notes" with **Update** (insert it at its place: the new note gets both
   links and its two neighbours get a new `next` / `previous`) and **Leave the chain**. Leave the
   chain writes the note without `previous` / `next` (as for an unticked match); the overview (6)
   does not depend on the chain and still shows it in the right place. An import never rewrites more
   than the two neighbours and the new note, and only their `previous` / `next` lines (the same
   line-level edit as a property edit in the properties card). Open notes with unsaved edits are saved
   first, as for a rename (VAULT-042).
6. **The thread overview** lives in the **right sidebar** as a view ("Thread", `panel:thread`, an
   icon of the right menubar, ADR-0014), which follows the active note: it lists every note with the
   same `thread-id` (a vault index built from the front matter and kept like the link index), oldest
   first, as one row per note with the date, the sender (`from` for a mail, the author for a post)
   and the note's title; the current note is marked (accent fill). A click opens the note through
   `ShaleApp::show_view` (so a pinned tab is not replaced, NAV-062). When the active note has no
   other note in its thread the view says so; without a `thread-id` it shows a short hint.
   **Keys** (scope Global, definable, KEY-001): *previous message of the thread*
   (`global.thread_previous`, Ctrl+Shift+PageUp), *next message of the thread*
   (`global.thread_next`, Ctrl+Shift+PageDown), *thread overview* (`global.thread_overview`,
   Ctrl+Shift+T; opens the right sidebar on the Thread view and gives it the keyboard: ↑ ↓ move,
   Enter jumps, Esc returns to the note). On macOS the same with Cmd. The steps use the overview's
   order (the date), not the `previous` / `next` links, so they also work when a chain was left alone;
   they do nothing at the ends or in a note without a thread, and not while a modal dialog is open
   (KEY-038). None is an Alt key and none is a key the system reserves (KEY-019); the clash check of
   KEY-001 must pass (a test checks it). Being Global they work in the views of the "Main views" scope
   (KEY-040) as well.
7. **Posts come after #560.** The thread view of a post in the Feeds tab (#560 on `dev/socials`) is
   what shows the conversation a reply belongs to and gives the importer the root and the parent id.
   The social part of #575 (SOC-070…SOC-072) is built after #560 has landed; the mail side and the
   overview do not wait for it, and the overview lists posts as soon as their notes carry the
   properties.
8. **Renames.** `previous` and `next` are wikilinks in front-matter properties, so a rename or move
   of a note rewrites them like every other link (VAULT-041, VAULT-042, one snapshot, VAULT-043). The
   chain survives a rename without extra code. `thread-id` and the message ids are not links and never
   change.

## Alternatives

- **A: `previous` / `next` only.** The smallest change, and the chain is plain wikilinks that every
  Markdown app shows. Rejected by the owner: with no root id the overview must walk the chain note by
  note, a gap in the chain (one note deleted or left out) cuts the thread in two, and a note imported
  before its parent has nothing to find it by. `thread-id` costs one line per note and fixes all
  three.
- **B: one `thread:` list per note** (a list of wikilinks to all notes of the thread, in order).
  Every note names the whole thread, so any app can show it without Shale. Rejected by the owner:
  every new message rewrites **every** note of the thread (N notes touched per import), which is a
  merge conflict for sync and a snapshot per note in the history, and the lists grow with the thread.
  C touches at most the new note and two neighbours.
- **Where the overview lives.** *A strip above the note* is always in sight but takes height from
  every note of a thread and gets long for 20 messages. *A popup* is quick but covers the note and
  cannot stay open while reading. A **right-sidebar view** keeps the list next to the note, scrolls,
  follows the note, costs no space for notes without a thread, and is a place the layout already has
  (ADR-0014); its cost is that the sidebar must be open (the overview key opens it). Chosen; the
  owner can overrule at review.
- **Matching by subject** (`Re:` stripped), by sender or by quoted text: rejected by the owner (wrong
  threads, and the headers say it exactly).

## Consequences

- An import writes **one note** and rewrites **at most two other notes**, and only their `previous`
  / `next` lines. A thread of N mails imported one by one is N new notes and N−1 appends to the
  previous last note.
- **Sync**: two devices importing parts of one thread at once can each rewrite the same neighbour;
  the conflict is the usual one of a note changed on two sides (a conflict copy, DATA-003), limited
  to two lines. The overview does not depend on the chain, so a lost `next` does not hurt it.
- **History** (ADR-0028): the import and the neighbour edits are one snapshot (reason `import`), so a
  restore of that snapshot takes the chain back.
- **Other Markdown apps** see `previous` and `next` as links (a quoted wikilink in a property) and
  the other ids as plain text; they show no overview, and nothing breaks without it.
- A vault with **old imports** has no `in-reply-to`, `references` or `thread-id`; a mail note with
  only `message-id` still matches as the **parent** of a new import (rule 3, first case). A new import
  changes an old note only by the chain rule (its `next`); no command rewrites the vault.
- `thread-id` is one more property name; the import writes it, the person can edit it (the overview
  then follows the new value).
- The overview needs a vault-wide index of `thread-id` (a front-matter scan like the one for tags),
  built with the link index so it adds no per-frame cost (PERF-001).
