Back to [[Start here]].

# Quick switcher

**Ctrl+O** finds a note by name, or creates it when nothing matches. A hint under the field shows the keys that move, open and close, as they are bound now.

# Search notes, mail and posts

**Ctrl+Shift+F** opens the search of the left sidebar. It is the same search as **Space f g**
(below), shown in the sidebar: it stays open while you work, is narrow, and lists up to 20 lines
of each note (the picker lists 3). The results are grouped under **Notes**, **Mail** (only while
your mail is set up and open) and **Posts** (the posts of your feeds that Shale loaded), each
group with its count (`200+` when there are more). `*` and `?` are wildcards, and the fields of
mail and posts work as in their own search (`from:`, `subject:`, `in:`, `date:`; see [[Mail]]
and [[Feeds]]).

## Choosing the sources: prefixes and buttons

A search looks in **notes, mail and posts**, and a generic word finds a lot in mail and posts. Two
things choose what is *listed*:

- **A prefix** at the very start of the query: `f:` notes, `m:` mail, `s:` posts, with or without a
  space after the colon (`f:banana`, `f: banana`). Letters combine: `fm:banana` looks in notes and
  mail. The prefix is for this query only and wins over the buttons. A word before the colon that
  is not made of these letters is just text: `from:ann` is still the field of the mail and post
  search. The letters are set in **Settings → Search** (one letter each; letters that clash with
  each other or that would spell a field such as `to:` or `in:` are refused).
- **The buttons** in the header, one per source with its count: **Notes 12 · Mail 20+ · Posts 4**
  (Mail only while your mail is open; `+` when there are more than the list holds). A source that
  is switched off is still counted, in grey, so you can see there is more. **Click** a button to
  list only that source; click it again to go back; click another to look at that one instead. So
  you can look at "what notes", then "what posts", without retyping. **Shift+click** adds a source
  to or removes it from the *default selection*, the one used when you have clicked nothing and typed
  no prefix; the last one cannot be switched off. The buttons never change what you typed.

The default selection is kept for the next time (also after a restart), and is the same in the sidebar
and in the picker; what a click soloes is not kept. **Tab** lists only the next source (Notes, Mail,
Posts, then back to the default selection) and **Shift+Tab** the previous one, in the box, while you
browse the sidebar results, and in the picker; the hint shows the keys, and they can be changed in
**Settings → Keybindings** (see [[Keyboard shortcuts]]).

↓ or Enter in the box browse the results: j / k (or ↓ / ↑) go from hit to hit through all
groups, Ctrl+J / Ctrl+K to the first hit of the next / previous note, and in mail and posts to
the first hit of the next / previous group. Enter (or a click) opens the hit where it belongs:
the note at the found text, the mail in the mail view, the post in a Feeds tab. Esc goes back to
the box. The list follows your typing a moment after you stop, and shows notes you edit meanwhile.

# The pickers (vim normal mode)

With vim-style editing on ([[Editor]]), the Space key starts these:

- **Space f f**: "Find a file" by its **name**. The letters you type must appear in order,
  ignoring case; ↓ / ↑ or Ctrl+J / Ctrl+K select, Enter opens the file in this tab, Esc closes.
  It never creates a note, unlike Ctrl+O.
- **Space f g**: "Search notes, mail and posts" in one list (the same search as the sidebar's), grouped under Notes, Mail and
  Posts with their counts. `*` (any characters) and `?` (one character) are wildcards. Enter
  opens the hit where it belongs: the note at the found line, the mail in the mail view, the post
  in a Feeds tab.

Both windows are bounded and never stall your typing. The search field and, right under it, a
hint with the keys stay at the top of the window; only the list below scrolls, however many hits
there are. The hint shows the keys as they are bound now: the four keys of these windows and of
the quick switcher (next line, previous line, open, close; ↓ / Ctrl+J, ↑ / Ctrl+K, Enter, Esc) can
be changed in **Settings → Keybindings** under "List windows", and the hint follows. The keys are
in [[Keyboard shortcuts]].

#userguide #search
