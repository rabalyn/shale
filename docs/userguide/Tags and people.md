---
features:
  - "People (contact notes)"
  - "Tags found automatically (no server, no AI)"
---
Back to [[Start here]]. This note is itself tagged: #userguide #tags #people.

# Tags

Tags are `#tag` in the text (nested `a/b` is fine, not inside links or code) or `tags:` in the
front matter of a note, as the notes made by [[Import]] have. The task list filters
by `#tag` ([[Tasks]]).

# Tags found automatically

- Tags of the vault that are written in the imported text are set right away (`#lecture-hall-b`
  for "Lecture Hall B").
- Shale also learns, from the notes in the vault, which words are typical for each tag, and
  suggests those tags when an import contains several of these words. It is plain counting, with
  no server and no AI: the same notes always give the same suggestions. It is learned again in
  the background now and then and kept in `.shale/tag-words.json` in the vault.

# People

![[people-picker.png]]

Contact notes live in the **people folder** (default `Adressbuch`; this guide uses `People`,
set in its `.shale/settings.json`; change it in Settings → Vault → People). An example is
[[Ada Example]].

- Type `[[p ` and a name to pick a person. The link that is written is a normal `[[Name]]`
  link, so other markdown tools understand it too. "New contact" makes a note from a template
  (e-mail, phone, organization, notes).
- When importing ([[Import]]), the sender, recipients, organizer and attendees are matched to
  contact notes (by e-mail address, else by name; "Müller, Hans" is "Hans Müller"). For ticked
  people without a note one is made, and the imported note gets a `people` property, so each contact
  note's backlinks show every mail and meeting with that person.

![[people-import.png]]

#userguide #tags #people
