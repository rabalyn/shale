---
features:
  - "Importing e-mails, events and files (for project tracking)"
---
# Import

Turn mails, events and files into notes. Back to [[Start here]].

![[import-dialog.png]]

- Drag e-mails from Thunderbird, events from a calendar, `.eml` or `.ics` files, or any other
  files (plans, PDFs, …) onto the window. The ribbon's Import button opens the same dialog; so
  do **Import to vault** in [[Mail]] and **Save to vault** in [[Feeds]].
- Choose the folder (one per project, for example) and tags. Small pictures from signatures are
  left out by default, and each attachment can be ticked on or off. Nothing is written until you
  press Import.
- Drop onto a folder in the file list to import into it, or onto a note to also add links to the
  new notes there.
- Messages that were imported before are recognised and left out unless you tick them again.

![[drag-event.png]]

Each e-mail becomes a note named "date subject" with sender, recipients and date, tags in its
properties, links to the attachments and the original message, and the text.

![[imported-note.png]]

People and tags are found automatically: [[Tags and people]]. On Wayland Shale reads what
Thunderbird hands over; when that is only a link to the message, Shale finds the message in
Thunderbird's local copy (IMAP accounts need "Keep messages for this account on this
computer").

#userguide #import
