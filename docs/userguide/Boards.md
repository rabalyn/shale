---
features:
  - "Kanban boards"
---
![[boards.png]]

A **board** shows your notes as cards in lanes, like a Kanban board. A lane fills itself from a query
(`status:doing`, `#todo`), so a card is a note, and when the note changes the card moves by itself. Back to
[[Start here]].

# Open the boards

Press **Space b** (in every main view, as **Space f f**, see [[Keyboard shortcuts]]; you can change the key in
Settings → Keybindings, "Show the boards"). The boards are a tab like [[Graph and backlinks]] and [[Tasks]], and
the tab comes back after a restart with the board it showed ([[Tabs]]).

At the top is a strip with every note of the vault that has `type: board` in its front matter; a click shows that
board. **+ New board** makes a note with one starter lane (`## To do`, `query: #todo`) in the folder of the open
board, or the one you are in, and shows it. **Edit file** opens the board's note in a tab.

# The board is a note

A board is a normal Markdown note: it opens in the editor, it is searchable and linkable, and other apps read it.
Its lanes are `##` headings, and the lines under a heading say what the lane shows:

```
---
type: board
card-shows: due, status, tags
---
# Sprint 48

## Backlog
query: #todo -#done
sort: priority desc, due asc

## Doing
query: status:doing
drop: status: doing

## Pinned by hand
- [[Release notes 0.9]]
- [[Kanban sketch.excalidraw]]
```

- `query:` is the query of the lane, in the language of [[Search syntax]] (`AND`, `OR`, `NOT`, `#tag`, `status:doing`,
  `due:`, `path:` and more). A lane matches whole notes, not lines. A lane without a query, with links, is a manual
  lane.
- `sort:` orders the cards: `priority desc, due asc`. The keys are `created`, `modified`, `due`, `priority` and `title`, each
  `asc` (the default) or `desc`. A note without a value for a key comes last, in either direction. Without a
  `sort:` line the cards come by title. `due` is the `due:` property, else the earliest open task date of the note;
  `created` and `modified` are the properties Shale keeps (see [[Notes and links]]), else the date in the file's name
  and the file's time.
- `drop:` is the property a card gets when it is dropped on the lane (`drop: status: doing` sets `status: doing`).
- `done: true` marks a lane of finished cards.
- `- [[Note]]` lines **pin** cards to a lane, in that order, above the others. A pinned card is only in the lane that
  pins it, even when the query of another lane matches it.
- `cards: every-lane` in the front matter shows a card in every lane that matches it. Without it a card is in the first
  lane that matches. The note of the board is never one of its cards.
- `card-shows: due, status, priority, tags` chooses what a card shows besides its title (all four without the line).

Lines the board does not know stay where they are when the view changes the file, and you can edit the file by hand:
the view follows.

# Lanes in the view

Each lane has a **⋯** menu: **Settings…**, **Rename**, **Move left**, **Move right** and **Delete lane**. **+ Add lane**
adds one at the end and lets you name it at once. Every change is written to the board's note. A **double click on the
heading** of a lane opens its settings: the same filter and sort form as in the search ([[Search and pickers]]), with
the **drop** property (filled in from a query like `status:doing`) and "Cards here are done". The **Board** menu at the
top right sets "A card in every lane it matches" and which chips the cards show.

A lane whose query is wrong shows what is wrong in its heading and stays empty; the other lanes work. A lane asks for
its cards on a background thread, so the window keeps drawing; when a note changes only the lanes it can change ask again.

# Cards

![[board-lane-settings.png]]

A card shows the note's title and chips: the **due** date (red when it is past), the **status**, the **priority**
(`priority:` in the front matter, from `highest` to `lowest`) and the **tags**. A drawing is a card too, with its picture;
it is found by name, path and links only.

- **Click** selects a card and opens its **peek**. **Ctrl+Click**, or **Enter** on the selected card, opens the note in
  a tab. The arrow keys move the selection (when no field has the keyboard).
- The **peek** is a small window next to the card, the size of the link preview (the setting "Link preview size", which dragging its edges also changes):
  a real editor of the note, which saves as the editor does. Drag its title to move it, its edges to resize it. The
  **Priority** control sets `priority:` in the front matter; it changes the front matter only, so `modified:` stays.
  **Open** shows the note in a tab, **×** or **Esc** closes the peek. With vim-style editing on, Esc first leaves insert
  mode, and **Space Space** ticks the task on the cursor's line.

![[board-peek.png]]

# Try it

This guide has a board of its own, [[Guide board]]: its lanes are made from the tags of the guide's notes. Open it with
**Space b**, change a query and watch the cards move.

#userguide #boards
