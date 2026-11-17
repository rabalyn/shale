---
features:
  - "Calendar"
---
![[calendar-month.png]]

A calendar in the right sidebar (a small month) and as a big tab, with Month, Week and Day
views in three sizes. Back to [[Start here]].

![[calendar-sidebar.png]]

## Calendars

Settings → Calendar → Calendars… adds local calendars (`.ics` files) or CalDAV accounts (for
example Nextcloud; "Find calendars" asks the server), chooses colours, shows or hides calendars
and sets the sync interval per calendar (15 minutes by default).

![[calendar-settings.png]]

## Events

![[calendar-week.png]]

- Click an event for its details: when, how it repeats, where, organizer, attendees,
  categories, reminders and notes. **Edit** and **Delete** work for local and CalDAV calendars.
  Delete asks first, and a copy is kept in `calendar-trash` in the data folder.
- A repeating event asks what you mean: **Edit this date** or **Edit all repeats**, **Delete
  this date** or **Delete all repeats**.
- If a local calendar's `.ics` file was deleted outside Shale, Delete shows an error ("the calendar
  file is gone") and writes nothing; the calendar is reloaded and shown empty. Saving an event
  creates the file again.
- Double-click a day or time to create an event; it starts at the next full or half hour and is
  30 minutes long.

![[event-dialog.png]]

- The event dialog sets how an event **repeats** (daily, weekly on chosen days, monthly,
  yearly; every N; ending never, on a date or after N times), the dates it **skips**,
  **reminders**, its status, busy or free, categories and the time zone. Shale stores and edits
  reminders but does not ring itself: your phone or another calendar program does.
- Moving an event's start moves its end along; changing the end never moves the start.
- "Save to" chooses any calendar. Moving an event writes it to the new calendar first and
  removes it from the old one only after that worked.
- **Nothing is lost when you save.** Shale writes back every line of an event that it read and
  you did not change.
- The first day of the week is Monday or Sunday (Calendars… dialog, `week_starts_monday`).

Events can also become notes: drag one onto a folder, see [[Import]].

#userguide #calendar
