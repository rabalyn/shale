Back to [[Start here]]. The windows and keys are in [[Search and pickers]], mail in [[Mail]],
posts in [[Feeds]].

Shale has one search for **notes, mail and posts**, and the three do not read what you type in
the same way. Notes look for your text as you typed it; mail and posts look for all your words,
in any order. This note says exactly what each one understands. On top of that, notes understand a
small **query language** (operators, parentheses and fields such as `#tag` and `status:doing`),
described under "The query language" below.

# Which source reads what

The text you type goes to every source that is switched on (see "Choosing the sources" below),
and each one reads it by its own rules:

| | Notes | Mail | Posts |
|---|---|---|---|
| What is searched | every line of every note, and the note's name | subject, sender, recipients, text, attachment names | text, author, links, date and the context of the post |
| The text is | a run of plain words is **one piece of text**, found inside a line; the query language (below) selects the notes first | **words**: all of them, in any order, each one **matches the start of a word** (`meet` finds "meeting") | the same |
| Case | ignored | ignored, also accents (`muller` finds `Müller`) | ignored, also accents |
| `*` and `?` | wildcards (any run of characters, one character) | only cut the word: every word is a **word start** anyway | only cut the word: every word is a **word start** anyway |
| `"phrase"` | the words inside the quotes, next to each other as typed; the quotes are no part of the text | words next to each other, in this order, **exactly** (no word starts) | the same |
| `from:` and the other fields | a property of the note (`from: ann` in its front matter), see the query language | limit a word to one field | limit a word to one field |
| Order of the list | the order of the files of the vault | newest first | newest first |

# Notes

A note is found when **one line** of it contains your text, spaces and all, ignoring
case (a run of plain words is one text; a field or an operator between words makes separate terms, see
"The query language"). The line is the hit; a note shows up to 3 lines in the picker and 20 in the sidebar. A note whose *name*
matches but none of its lines is one hit by its name. Leading and trailing spaces of the query
are ignored.

| You type | It finds |
|---|---|
| `banana` | every line with "banana", "Banana", "bananas" |
| `red apple` | lines with "red apple" in this order, with one space. "apple red" is another search; so is a line that has "red" and "apple" far apart |
| `ban*a` | "banana", "ban and a" (`*` is any run of characters, also none) |
| `b?nana` | "banana", "bonana" (`?` is exactly one character) |
| `*` or `??` | a syntax error: a text of wildcards alone would match everything |
| `from:ann` | notes whose front matter has `from: ann` (a property, not text) |
| `"to do"` | lines that contain `to do`; the quotes are no part of the text, and inside them `AND`, `(` and `-` are plain text |
| `cats OR dogs` | notes with a line that has "cats" or a line that has "dogs" (an operator, upper case) |
| `cats or dogs` | lines that contain the sentence "cats or dogs": lower case is text |

The found text is marked in the hit. In mail and posts a word marks where a word of the text starts with it. For a wildcard the mark runs from where the pattern starts
to where it ends (`*` matches as little as it can: `ban*a` marks "Bana" in "Banana"). A match
cannot continue into the next line, and you cannot search for a literal `*` or `?`.

# The query language

The search of everything (the sidebar and **Space f g**) reads what you type as one language, the one
the lanes of a board will use too. A query is checked before it runs: when it is not valid, the wrong part
is underlined with its reason below the box, nothing is searched and the last list stays.

| You type | It means |
|---|---|
| `a b` | two plain words side by side are one text, the phrase "a b" |
| `a AND b` | a note that has "a" and has "b", also on different lines. Two terms side by side are `AND` too: `#todo milk` |
| `a OR b` | one of them |
| `NOT a`, `-a` | notes without "a". `-(a OR b)` negates a group |
| `( … )` | groups; `AND` binds tighter than `OR`, `NOT` tighter than `AND`: `a OR b AND c` is `a OR (b AND c)` |
| `#tag` | notes with the tag (also in the front matter `tags:`); `#project` finds `#project/alpha` too |
| `status:doing` | notes whose front matter has the property `status` with the value `doing` (case ignored; a list matches when one item is the value). Quote a value with spaces: `status:"in progress"` |
| `priority:high` | the same; the values are `highest`, `high`, `medium`, `low`, `lowest`, anything else is an error |
| `path:Projects/` | notes whose path starts with this |
| `due:2026-12-01` | the `due:` property is this day |
| `due:<+7d`, `due:>-3d` | due before 7 days from today, after 3 days ago. `<=` and `>=` include the day. `due:<today` works too |
| `due:overdue` | due before today |
| `links:[[Note]]` | notes that link to Note |
| `blocked` | the note has `blocked-by: [[A]], [[B]]` and one of them exists and is not `status: done` (on a board the cards in a done lane do not count as done here, see [[Boards]]) |
| `blocking` | the note is not done and another note that is not done names it in `blocked-by:` |
| `orphan` | a note or drawing with no link in or out: a link must reach a note or drawing of the vault, so a link to itself, a red link, and a link to an image, a PDF or other media do not count |
| `no-backlinks` | a note or drawing that no other note or drawing links to (it may link out; a link to itself does not count). `orphan AND NOT no-backlinks` is empty, and `no-backlinks AND NOT orphan` finds the notes that only link out |
| `"AND"` | a quoted word is text: `"AND"`, `"#not a tag"`, `"blocked"`, `"orphan"` |

You do not have to type this: the **gear** form next to the search box builds the same queries from rows and
reads them back (see [[Search and pickers]]); a query with more than one level of `AND` and `OR` is shown as text.

`AND`, `OR` and `NOT` are operators **only in upper case**: "salt and pepper" is still the phrase. A
word with a colon is a property when it starts with a letter and has a value (`12:30`, `http://x` and
`TODO:` are text). The words `blocked`, `blocking`, `orphan` and `no-backlinks` are fields; quote them to search for the word.

**Fields select notes, text finds lines.** `status:doing banana` lists the lines with "banana" in
the notes that are `doing`; `status:doing` alone lists the notes. A text under `NOT` rules notes out but
shows no lines. Mail and posts understand text and their own fields only (`from:`, `subject:`, …), side
by side: for a query with `#tag`, `status:`, `OR` or the like the search skips them and says so under the
source buttons ("Mail: `status:` not supported"), it does not show them as empty.

**Limits.** A query has at most 500 characters, 32 terms, 8 levels of parentheses and `NOT`, and a text has at most 8
`*`. A search also has a budget of work and time: a pattern such as `*a*a*a*a*b` over a long line is
expensive, so when the budget is used up the search stops, shows what it found, and says "Stopped early,
narrow the query".

# Mail

Mail is searched over **all folders**, with a full-text index. Every word you type must be in the
mail, anywhere in it, in any order. Every word matches the **start of a word**:
`meet` finds meet, meets and "meeting", but not "comet" (not the middle of a word). A "phrase" stays exact.

| You type | It finds |
|---|---|
| `invoice` | mails with the word "invoice" in any field |
| `invoice march` | mails that have both words, in any order, also far apart |
| `"annual report"` | mails with these two words next to each other, in this order |
| `meet` | words that start with "meet": meeting, meetings, meet (the same as `meet*`) |
| `"meet"` | only the word "meet": a phrase is exact |
| `mee?ing` | the same as `mee*`: the word is cut at the first wildcard and what is left is a prefix, so `?` finds more than it says |
| `*ing` | nothing from this word: a word that starts with a wildcard is dropped (the rest of the query still counts) |
| `invoice AND march` | needs the words "invoice", "and" and "march". `AND`, `OR`, `NOT` are plain words, and `NEAR` is too |
| `invoice -spam` | needs "invoice" and "spam": the `-` is dropped, it does not exclude |
| `from:anna` | a word of the sender starts with "anna" |
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
order, case and accents ignored, every word a word start (`meet` finds "meeting"), wildcards only cut the word, `"phrases"`, and `AND`, `OR`, `NOT`
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

- `from:ann` in mail and posts limits "ann" to the sender or the author. In notes it is a property
  (the front matter `from: ann`).
- `from:` and `body:` exist in mail and posts, `to:`, `cc:` and `subject:` only in mail, `text:`,
  `in:`, `context:`, `link:` and `date:` only in posts. A field word of the other kind is plain
  text there: in posts `subject:budget` is a field posts do not have, so the search skips the posts and
  says so ("Posts: `subject:` not supported").
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
| Sidebar search | **Ctrl+Shift+F** | notes, mail and posts, in groups with counts; up to 20 lines per note, 200 hits per group | all of this note, including the query language; the sources and buttons as above |
| Search picker | **Space f g** (every view, [[Search and pickers]]) | the same search in a window; 3 lines per note, 20 hits per group | the same |
| File picker | **Space f f** | **file names** only: the letters you type must be in the name in this order | letters in order, no wildcards, no fields; never creates a note |
| Quick switcher | **Ctrl+O** | note names, like the file picker | letters in order; creates the note when nothing matches |
| The search of the mail view | **/** in the mail view, or the field above the list | mail only, **all folders** or **this folder** (**t** switches) | the same as mail above: words as word starts, wildcards, `"phrases"` and the five fields. The empty field reminds you of them |

Mail is part of the sidebar and the picker only while your mail is set up and open. In every
list of hits, Enter or a click opens the hit where it belongs: the note at the found text, the
mail in the mail view, the post in a Feeds tab.

# How the hits look

A found line of a note is shown **rendered** (bold, links by their shown text, a task with a checkbox, a
heading without its `#`) or as **raw markdown**, and the **found text is marked** with a
background in both. Mail and post hits are plain text, and the words of your query are marked in
the subject or the start of the text; a mail found only by a word in its text may show no mark. At most 120 characters of a line are shown.

**Settings → Search → Hits of notes** chooses: **Follow the editor** (the default: rendered while
the Markdown preview is on or the note you are in is in the [[Reading view]], raw otherwise),
**Raw markdown** or **Rendered**.

#userguide #search
