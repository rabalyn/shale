Back to [[Start here]].

# Quick switcher

**Ctrl+O** finds a note by name, or creates it when nothing matches. A hint under the field shows the keys that move, open and close, as they are bound now. Typing part of "Switch vault" lists that command too: it shows the list of your vaults (the same as the vault switcher at the bottom of the left sidebar, see [[Vault settings]]).

# Search notes, mail and posts

**Ctrl+Shift+F** opens the search of the left sidebar. It is the same search as **Space f g**
(below), shown in the sidebar: it stays open while you work, is narrow, and lists up to 20 lines
of each note (the picker lists 3). The results are grouped under **Notes**, **Mail** (only while
your mail is set up and open) and **Posts** (the posts of your feeds that Shale loaded), each
group with its count (`200+` when there are more). What you can type (wildcards, every word of mail and posts as a word start, `"phrases"`,
the fields `from:`, `subject:`, `in:`, `date:` of mail and posts) is in [[Search syntax]].

The search reads a small **query language**: `AND`, `OR`, `NOT` (upper case), parentheses, `-word`,
and the fields `#tag`, `status:doing` (any property of a note), `path:Projects/`, `due:<+7d`,
`due:overdue`, `links:[[Note]]`, `blocked`, `blocking`, `orphan` and `no-backlinks`. For example `#todo due:overdue NOT status:done`
lists the notes with the tag whose due date has passed. A query that is not valid (an open parenthesis,
`due:soon`) is underlined in the box with the reason under it, and the last list stays. Mail and posts
take only words and their own fields; when a query has something they do not understand, a line under the
source buttons says which source was skipped. When a search takes too long it stops and says "Stopped
early, narrow the query". The rules are in [[Search syntax]].

![[sidebar-search.png]]

## The filter form (the gear button)

Next to the search box, in the sidebar and in the picker, is a **gear** button. It opens a **filter form** under
the box, for building a query without remembering the syntax: **All of** (every row must match) or **Any of** (one
of them), and rows you add with **+ Add filter**: **tag is**, **property is** (a name and a value), **due** (overdue,
or within, before, on, after or on or after a day: a date, `today` or `+7d`), **is blocked**, **is blocking**, **is an orphan**, **has no backlinks**,
**in folder**, **links to** and **text contains**. Each row has a **not** switch and a ✕ to remove it. The form writes
the query into the box as you go and the list follows at once; what you type in the box fills the form. A row
that is not complete yet (an empty tag, a property without a value) is not in the query and says what it needs.

**Suggestions.** The tag field, the folder field, the name of a property and its value suggest what the vault knows: the tags, the folders that hold notes (nested ones too, written with a trailing `/`), the
names of the properties in the notes' front matter, and for the name you chose the values seen with it. Click into an empty
field and the list shows everything, alphabetically; type and it narrows (what starts with your letters first, then
what contains them). **↓ / ↑** (or Ctrl+J / Ctrl+K) choose, **Enter** or **Tab** take the entry, a click takes it too,
and **Esc** closes only the list. A long list scrolls. `created` and `modified` are not suggested. In a very large vault the first list may say
"Loading…" for a moment while the suggestions are collected in the background; the form stays usable, and the list
fills in by itself.

![[picker-form.png]]

**Advanced** shows the query as text instead. In the sidebar and the picker the text is the box above, and Advanced adds
the live count of matches ("12 matches"); in a board lane it is a text field of its own with the error underlined. The
last choice is remembered (**Settings** file, `search.advanced`) and is the same in the sidebar, the picker and the boards.

A query that does not fit the rows (parentheses inside an `OR`, a `NOT` of a group) is not changed: the form says "This
query is too complex for the form", Advanced stays on and you edit the text. A `f:`, `m:` or `s:` prefix stays where it is.
The form is reached with the keyboard: **Ctrl+Shift+G** opens and closes it (the key can be changed in **Settings →
Keybindings**, "List windows" and "Search results"), **Tab** walks through its widgets, and **Esc** closes an open list or menu first and then the picker.

![[sidebar-form.png]]

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

## How a hit of a note looks

A found line of a note is shown **rendered**, the way the editor draws a line that the cursor is not
on: bold, italic, strikethrough, highlight and inline code styled, links and wiki links by the text
they show (`[[Note|alias]]` is "alias"), `#tags` in the tag colour, a heading in the heading style
without its `#`, a task with a checkbox (ticked when it is done) and a list item with a bullet. The **found text is
marked** with a background, in the rendered and in the raw line alike; when a match covers markup that
is not drawn, the visible part of it is marked. Mail and post hits stay plain text and mark the words
of your query in their subject or text.

**Settings → Search → Hits of notes** chooses the look: **Follow the editor** (the default) draws the
lines rendered while the Markdown preview is on ([[Editor]]) or the note you are in is in the
[[Reading view]], and as raw markdown otherwise; **Raw markdown** and **Rendered** always draw one
of them. Changing it, the preview or the view changes the open results at once. A line with a
formula is shown raw, and a hit shows at most 120 characters of its line.

↓ or Enter in the box browse the results: j / k (or ↓ / ↑) go from hit to hit through all
groups, Ctrl+J / Ctrl+K to the first hit of the next / previous note, and in mail and posts to
the first hit of the next / previous group. Enter (or a click) opens the hit where it belongs:
the note at the found text, the mail in the mail view, the post in a Feeds tab. Esc goes back to
the box. The list follows your typing a moment after you stop, and shows notes you edit meanwhile. The search runs in the background: while a new list is being made the last one stays, with a small spinner next to the source buttons, and typing is never held up. A search that takes longer than two seconds is also listed in the indicator in the corner as "Searching…", where Cancel stops it and keeps the last list.

# The pickers (Space keys)

The Space key starts these in every main view: the editor (in vim normal mode, [[Editor]]), the reading
view, mail, the calendar, Feeds, tasks and the graph. They do nothing while a text field has the keyboard
(a search box, a mail you write, a note in insert mode), under a dialog, or in a drawing (Space pans). In
the mail view the single keys (j, k, f, d, …) keep working; only the mark moved from Space to **x**. Both can
be changed in Settings → Keybindings ("Main views").

- **Space f f**: "Find a file" by its **name**. The letters you type must appear in order,
  ignoring case; ↓ / ↑ or Ctrl+J / Ctrl+K select, Enter opens the file in this tab, Esc closes.
  It never creates a note, unlike Ctrl+O.
- **Space f g**: "Search notes, mail and posts" in one list (the same search as the sidebar's), grouped under Notes, Mail and
  Posts with their counts; the syntax and the query language are in [[Search syntax]]. Enter
  opens the hit where it belongs: the note at the found line, the mail in the mail view, the post
  in a Feeds tab.

![[picker-search.png]]

Both windows are bounded and never stall your typing. The search field and, right under it, a
hint with the keys stay at the top of the window; only the list below scrolls, however many hits
there are. The hint shows the keys as they are bound now: the four keys of these windows and of
the quick switcher (next line, previous line, open, close; ↓ / Ctrl+J, ↑ / Ctrl+K, Enter, Esc) can
be changed in **Settings → Keybindings** under "List windows", and the hint follows. The keys are
in [[Keyboard shortcuts]].

#userguide #search
