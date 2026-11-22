Back to [[Start here]]. The windows and keys are in [[Search and pickers]], mail in [[Mail]],
posts in [[Feeds]].

Shale has one search for **notes, mail and posts**, and the three do not read what you type in
the same way. Notes look for your text as you typed it; mail and posts look for all your words,
in any order. This note says exactly what each one understands.

# Which source reads what

The text you type goes to every source that is switched on (see "Choosing the sources" below),
and each one reads it by its own rules:

| | Notes | Mail | Posts |
|---|---|---|---|
| What is searched | every line of every note, and the note's name | subject, sender, recipients, text, attachment names | text, author, links, date and the context of the post |
| The text is | **one piece of text**, found inside a line | **words**: all of them, in any order | **words**: all of them, in any order |
| Case | ignored | ignored, also accents (`muller` finds `Müller`) | ignored, also accents |
| `*` and `?` | wildcards (any run of characters, one character) | only make the word a **prefix** | only make the word a **prefix** |
| `"phrase"` | the quotes are part of the text | words next to each other, in this order | words next to each other, in this order |
| `from:` and the other fields | plain text, no field | limit a word to one field | limit a word to one field |
| Order of the list | the order of the files of the vault | newest first | newest first |

# Notes

A note is found when **one line** of it contains your whole text, spaces and all, ignoring
case. The line is the hit; a note shows up to 3 lines in the picker and 20 in the sidebar. A note whose *name*
matches but none of its lines is one hit by its name. Leading and trailing spaces of the query
are ignored.

| You type | It finds |
|---|---|
| `banana` | every line with "banana", "Banana", "bananas" |
| `red apple` | lines with "red apple" in this order, with one space. "apple red" is another search; so is a line that has "red" and "apple" far apart |
| `ban*a` | "banana", "ban and a" (`*` is any run of characters, also none) |
| `b?nana` | "banana", "bonana" (`?` is exactly one character) |
| `*` or `??` | nothing: a text of wildcards alone finds nothing |
| `from:ann` | lines that contain "from:ann". It is no field here |
| `"to do"` | lines that contain the quotes as well |
| `AND`, `OR`, `NOT`, `-word` | plain text, like any other word: `cats OR dogs` looks for that sentence |

The found text is marked in the hit. For a wildcard the mark runs from where the pattern starts
to where it ends (`*` matches as little as it can: `ban*a` marks "Bana" in "Banana"). A match
cannot continue into the next line, and you cannot search for a literal `*` or `?`.

# Mail

Mail is searched over **all folders**, with a full-text index. Every word you type must be in the
mail, anywhere in it, in any order. Words are whole words, not parts of words: `meet` does not
find "meeting" unless you type `meet*`.

| You type | It finds |
|---|---|
| `invoice` | mails with the word "invoice" in any field |
| `invoice march` | mails that have both words, in any order, also far apart |
| `"annual report"` | mails with these two words next to each other, in this order |
| `meet*` | words that start with "meet": meeting, meetings, meet |
| `mee?ing` | the same as `mee*`: the word is cut at the first wildcard and what is left is a prefix, so `?` finds more than it says |
| `*ing` | nothing from this word: a word that starts with a wildcard is dropped (the rest of the query still counts) |
| `invoice AND march` | needs the words "invoice", "and" and "march". `AND`, `OR`, `NOT` are plain words, and `NEAR` is too |
| `invoice -spam` | needs "invoice" and "spam": the `-` is dropped, it does not exclude |
| `from:anna` | the sender contains "anna" |
| `from:"Anna M"` | the sender contains the phrase "Anna M" |
| `to:team subject:budget` | recipient "team" and "budget" in the subject |
| `cc:finance` | someone in Cc |
| `body:deadline` | the word only in the text of the mail |

The fields are **`from:`**, **`to:`**, **`cc:`**, **`subject:`** and **`body:`**. They are
written in lower case or upper case, followed at once by the word or by a "phrase". A word
before the colon that is not one of these is plain text. A search holds at most 24 words and 512 characters.
Attachment names are searched too, but there is no field for them.

# Posts

The posts of your feeds that Shale loaded ([[Feeds]]) are searched like mail: all words, in any
order, case and accents ignored, wildcards only as prefixes, `"phrases"`, and `AND`, `OR`, `NOT`
and `-` as plain text. The search always looks at all your accounts.

| You type | It finds |
|---|---|
| `rust release` | posts that have both words |
| `"new release"` | posts with these words next to each other |
| `rele*` | posts with a word that starts with "rele" |
| `from:ada` | posts by an author whose name or handle contains "ada" |
| `text:release` or `body:release` | the word is in the text of the post (`body:` is the same as `text:`) |
| `in:ada` or `context:ada` | the word is in the context of the post: the post it answers, a quoted post, a link card, who boosted it (`context:` is the same as `in:`) |
| `link:example.org` | the post has a web address with this word |
| `date:2026-11` | posts of that month; `date:2026-11-15` of that day (UTC) |
| `from:ada date:2026-11 release` | all three at once |

The fields are **`from:`**, **`text:`** (or **`body:`**), **`in:`** (or **`context:`**),
**`link:`** and **`date:`**.

# Fields belong to mail and posts

One query goes to all sources, so a field word is read differently by each:

- `from:ann` in mail and posts limits "ann" to the sender or the author. In notes it is plain
  text, so notes with the line "from:ann" show up in the Notes group.
- `from:` and `body:` exist in mail and posts, `to:`, `cc:` and `subject:` only in mail, `text:`,
  `in:`, `context:`, `link:` and `date:` only in posts. A field word of the other kind is plain
  text there: in posts `subject:budget` needs the words "subject" and "budget" in the post.
- To get rid of the notes, start the query with a source prefix (next section): `ms:from:ann`.

# Choosing the sources

A generic word finds a lot in mail and posts. Two things choose which sources are *listed*; both
search and count all of them, and neither changes what you typed.

| What | How it works |
|---|---|
| Prefix `f:` `m:` `s:` | at the very start of the query: `f:` notes, `m:` mail, `s:` posts. A space after the colon is allowed (`f: banana`). It wins over the buttons and applies to this query only. |
| Combined | letters in any order, each once: `fm:banana` is notes and mail, `ms:` is mail and posts. |
| Not a prefix | a word before the colon with other letters (`from:ann`), a prefix that is not at the start (`a f:x`), a letter twice (`ff:x`). It stays text. |
| Count buttons | **Notes 12 · Mail 20+ · Posts 4** under the field. A **click** lists only that source; a click on the same button again goes back; a click on another one looks at that one instead. The count is `20+` (sidebar `200+`) when there are more. The Mail button shows only while your mail is open. |
| Shift+click | adds a source to or removes it from the **default selection** (what is listed when you clicked nothing and typed no prefix). The last one cannot be switched off. |
| Tab / Shift+Tab | list only the next / previous source: Notes, Mail, Posts, then the default selection. They do nothing while the query has a prefix. |

The default selection is saved and the same in the sidebar and in the picker; a click on one
button (a "solo") is not saved. The three letters can be changed in **Settings → Search**: one
letter each, and letters that clash, or that could spell a field such as `to:` or `in:`, are
refused. Tab and Shift+Tab can be changed in **Settings → Keybindings** ("List windows" and
"Search results", [[Keyboard shortcuts]]).

# Where to search

| Where | Keys | What it searches | Syntax |
|---|---|---|---|
| Sidebar search | **Ctrl+Shift+F** | notes, mail and posts, in groups with counts; up to 20 lines per note, 200 hits per group | all of this note; the sources and buttons as above |
| Search picker | **Space f g** (vim-style editing on, [[Editor]]) | the same search in a window; 3 lines per note, 20 hits per group | the same |
| File picker | **Space f f** | **file names** only: the letters you type must be in the name in this order | letters in order, no wildcards, no fields; never creates a note |
| Quick switcher | **Ctrl+O** | note names, like the file picker | letters in order; creates the note when nothing matches |
| The search of the mail view | **/** in the mail view, or the field above the list | mail only, **all folders** or **this folder** (**t** switches) | words, `"phrases"` and the five fields as above. **No wildcards**: `meet*` is the word "meet" |

Mail is part of the sidebar and the picker only while your mail is set up and open. In every
list of hits, Enter or a click opens the hit where it belongs: the note at the found text, the
mail in the mail view, the post in a Feeds tab.

# How the hits look

A found line of a note is shown **rendered** (bold, links by their shown text, a task as `[ ]`, a
heading without its `#`) or as **raw markdown**, and the **found text is marked** with a
background in both. Mail and post hits are plain text, and the words of your query are marked in
the subject or the start of the text; a mail found only by a word in its text may show no mark. At most 120 characters of a line are shown.

**Settings → Search → Hits of notes** chooses: **Follow the editor** (the default: rendered while
the Markdown preview is on or the note you are in is in the [[Reading view]], raw otherwise),
**Raw markdown** or **Rendered**.

#userguide #search
