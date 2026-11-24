# ADR-0038: Mail, Calendar and Feeds as compact companions in the right sidebar

- **Status**: Accepted (2026-11-24); step 1 (the shared placing and Mail) is built, Calendar and Feeds follow
- **Date**: 2026-11-24
- **Specs**: LAY-028…LAY-034
- **Issues**: rabalyn/shale#610
- **Amends**: [ADR-0033](0033-views-on-either-side.md) (its last consequence, which refuses Mail, Feeds and Calendar in the sidebar, and decision 4, one state for a view shown twice)

## Context

ADR-0033 let Files, Search and Tags live on either side and refused Mail, Calendar and Feeds:
they are tabs with their own state and size needs, and "cannot become sidebar panes without a new
decision". The owner has taken that decision (2026-11-23): like the other views, they should be
placeable in the sidebar, not only as main views. Mail is the one a person wants to keep an eye on
while writing a note; the Calendar and the home timeline of Feeds likewise.

The three views do not fit a narrow column as they are: Mail has three panes (folders, list,
message), the Calendar a grid and a side bar, Feeds a column of tabs and a thread pane.

## Decision

1. **A compact companion, not a squeezed full view.** What a right view shows of each is made for
   a column of 200 to 560 points and is a way into the main view, not a copy of it:
   - **Mail**: the accounts, the folders of the chosen account (with their unread counts) and the
     message list of the chosen folder, with the unread state. A click on a message opens it in
     the Mail tab of the main area (opening the tab when there is none). Reading, writing,
     searching, moving and the settings stay in the tab.
   - **Calendar**: a small month and the agenda of the next days. A click on an event opens it in
     the Calendar tab.
   - **Feeds**: the home timeline in one column. A click opens the thread in the Feeds tab.
2. **Independent view state.** The pane has its own account, folder or feed selection, its own
   selected item and its own scroll, separate from the main tab of the same kind: the sidebar can
   watch the Inbox while the tab shows another folder. Only the **view state** is per place. The
   **data**, the caches and the background work (the index, the syncs, the watch, the feed
   fetches, the calendar sources) are the app's one set: the pane reads what the tab reads, never
   starts a second sync, and shows a change made in the tab (a mail read there, a post fetched)
   after the next read of the shared data. The state of the pane is not saved: a new window opens
   it at its first account, folder or feed.
3. **Placing is the same as for Files, Search and Tags** (ADR-0033, decisions 2, 3, 6 and 7): the
   drag from the ribbon to a right menubar or to the right sidebar's content asks "Move or copy?"
   (Move, Copy, Cancel, Esc), a drag back to the ribbon likewise, and the right-click menu has
   Move to / Copy to the right (left) sidebar. **Copy keeps the ribbon icon, and the ribbon icon
   still opens the main tab** (a click, and a middle click in a new tab, LAY-017). Move takes the
   icon from the ribbon; the view is then reached through the sidebar, the shortcuts (Ctrl+Shift+M
   for Mail). **The left sidebar never shows these three as panes**; it
   still holds Files, Search and Tags only. They are stored in the same `sidebars` ids (`mail`,
   `calendar`, `social`) in `sidebars.right[].icons`, with the repair rules of ADR-0033 decision 6;
   an older Shale reading such a file puts the ids back in the ribbon.
4. **Built in steps.** Step 1 makes the placing work for all views that have a pane and builds
   the Mail pane. An icon that has no pane yet is refused at the drop like before (no-drop cue,
   no menu entries) and a stored one in a right view goes back to the ribbon, so nobody can place
   an empty pane. Step 2 adds the Calendar pane and step 3 the Feeds pane; each only adds its
   pane and its state to the set of views that can be placed, no change of the placing.

## Why the state is independent here and shared for Files, Search and Tags

ADR-0033 (decision 4) shares one state between the two copies of a view: a second file tree, a
second search with its own background search and a second tag list cost memory and work for
nothing, and a person who copies Search to the right wants to see the same search. The three
views here are different:

- Their state is a **place in a large body of data** (an account and a folder, a month, a feed),
  which is cheap: a number or a date. The expensive part, the data, is shared anyway.
- A person uses the sidebar companion **beside** the tab, not as a second window onto the same
  place: the Inbox in the sidebar while the tab shows a project folder, today's agenda while the
  tab shows next month. With one shared state the sidebar would follow every click in the tab and
  could not stay on what it is there to watch.
- Opening something from the sidebar changes the tab (that is the point of the click), so the
  tab must not also be the sidebar's own selection.

Alternative considered: one shared state, like Files. Refused for the reasons above: the pane
would lose the place it was put there to hold.

## Consequences

- "An icon exists once" holds for every icon except the movable views, which are now Files, Search,
  Tags and Mail (later also Calendar and Feeds).
- The window has a small container of view states for these panes (`SideState::panes`), one field per
  view; a copy of Files, Search and Tags has no entry there.
- The data stays in `MailView`, which counts how often it read the index (`refreshed`), so a pane
  that keeps pages of the list reads them again when the index changed.
- The pane opens the store like the tab does (it needs an account in the settings) but the tab's
  watch of the open folder and the Inboxes (MAIL-230) runs only while the tab is shown, so a
  sidebar alone shows what the last sync and the automatic sync brought.
- Taking Mail out of the ribbon (Move) takes away the ribbon's way into the tab; that is the
  owner's choice and "Restore" and Copy bring it back.
