---
features:
  - "Calendar"
---
![[calendar-month.png]]

A calendar in the right sidebar (a small month) and as a big tab, with Month, Week and Day
views in three sizes. In a narrow sidebar the buttons move to a line below the month name instead of covering it. Back to [[Start here]].

![[calendar-sidebar.png]]

The calendar tab comes back after a restart in the mode (month, week or day) and on the day you left it ([[Tabs]]).

# Calendars

Settings → Calendar → Calendars… adds local calendars (`.ics` files) or CalDAV accounts (for
example Nextcloud; "Find calendars" asks the server), chooses colours, shows or hides calendars
and sets the sync interval per calendar (15 minutes by default).

**Edit** on a CalDAV calendar changes its address, user name or password without removing it: leave
the password empty to keep the saved one. A new user name or password also goes to your other
CalDAV calendars on the same server with the same old user name, and the dialog says how many
calendars it changed. Name, colour and sync interval stay, and the calendar is loaded again.
**Save** applies it to the list, **Done** writes it; **Cancel** drops the change. Local calendars
have no Edit button.

![[calendar-settings.png]]

# Events

![[calendar-week.png]]

- Click an event for its details: when, how it repeats, where, organizer, attendees,
  categories, reminders and notes. **Edit** and **Delete** work for local and CalDAV calendars.
  Delete asks first, and a copy is kept in `calendar-trash` in the data folder.
- **Import** (in the details, and in the right-click menu of an event) turns the event into a
  note, the same as dragging it out of the calendar: the Import dialog opens, see [[Import]]. For
  a repeating event it imports the date you clicked.
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

Events can also become notes: use **Import**, or drag one onto a folder, see [[Import]].

#userguide #calendar
