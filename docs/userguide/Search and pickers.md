Back to [[Start here]].

## Quick switcher

**Ctrl+O** finds a note by name, or creates it when nothing matches.

## Search all notes

**Ctrl+Shift+F** opens the search of the left sidebar. ↓ or Enter in the box browse the results:
j / k (or ↓ / ↑) go from hit to hit, Ctrl+J / Ctrl+K from file to file, Enter opens the note at
the hit, Esc goes back to the box. Posts of your feeds that Shale loaded are listed under the
notes; see [[Feeds]] for the filters (`from:`, `in:`, `date:`).

## The pickers (vim normal mode)

With vim-style editing on ([[Editor]]), the Space key starts these:

- **Space f f**: "Find a file" by its **name**. The letters you type must appear in order,
  ignoring case; ↓ / ↑ or Ctrl+J / Ctrl+K select, Enter opens the file in this tab, Esc closes.
  It never creates a note, unlike Ctrl+O.
- **Space f g**: "Search notes, mail and posts" in one list, grouped under Notes, Mail and
  Posts with their counts. `*` (any characters) and `?` (one character) are wildcards. Enter
  opens the hit where it belongs: the note at the found line, the mail in the mail view, the post
  in a Feeds tab.

Both windows are bounded and never stall your typing. The keys are in [[Keyboard shortcuts]].

#userguide #search
