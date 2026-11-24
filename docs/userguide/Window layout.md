---
features:
  - "The ribbon and the sidebars"
---
![[sidebar-split.png]]

Back to [[Start here]]. The ribbon is the narrow bar on the left; the right sidebar is a stack
of **views**.

# The ribbon

- Drag the icons into any order; a line shows where the icon lands. Esc, or letting go anywhere
  else, cancels. The sidebar toggles and the settings button stay where they are.
- A middle click on **Mail**, **Feeds** or **Calendar** opens that view in a new tab
  ([[Mail]], [[Feeds]], [[Calendar]]).
- The settings button at the bottom opens Settings; its **Help** section has **Open user guide**
  (F1), which opens this guide (see [[Start here]]).
- Right-click an icon: **Remove from the ribbon** / **Remove from this view**, **Restore …** for
  every icon removed earlier, and **Reset layout**. The last icon cannot be removed.

# The right sidebar

- Each view has its own menubar of panel icons (Backlinks, Outgoing links, Outline, Graph,
  Calendar, History, Thread) and shows the panel of the selected icon, so several panels can be open at once.
- Drag a panel icon over the right sidebar ("Drop to open this icon in a new view") to make a
  new view below it. Drag icons between menubars, to the ribbon (a shortcut that shows the panel
  below the views) and back. Icons that are no panels and no views (Graph, Tasks, …) cannot go there. Taking
  the last icon out of a view removes the view. Esc cancels a drag.
- **Files, Search and Tags can be on either side.** Drag the icon of Files, Search or Tags from the ribbon to
  the menubar or the content of the right sidebar (or back from a right menubar to the ribbon). When you let go,
  Shale asks **Move or copy?**: **Move** takes the view from the side it came from, **Copy** shows it on both sides
  (the same file tree or search, with the same state), **Cancel** (or Esc) changes nothing. Without dragging, right-click
  the icon and choose **Move to the right sidebar** or **Copy to the right sidebar** (in a right menubar: to the left
  sidebar). If the view the left sidebar showed has moved, it shows the next one that is still in the ribbon.
- **Mail can be in the right sidebar too.** Place the Mail icon like Files, Search or Tags (drag it to a right menubar or to the
  right sidebar's content and answer **Move or copy?**, or right-click it and choose **Move to the right sidebar** or
  **Copy to the right sidebar**). The pane is a compact companion of the Mail tab: the accounts, the folders with their unread
  counts and the list of messages with the unread dots. Click a message to open it in the Mail tab (Shale opens the tab if
  there is none). The pane has its own folder choice and scroll, separate from the tab, so it can keep showing the Inbox
  while the tab shows another folder; the mail itself and the sync are the same. With **Copy** the ribbon keeps its Mail
  icon, which still opens the tab; the left sidebar never shows Mail.
- **Calendar can be in the right sidebar too.** Place the Calendar icon the same way (not the *Calendar* panel icon, which
  is a different, older view with its own month). The pane is a compact companion of the Calendar tab: a small month with ‹ ›
  and **Today** (today is marked, a dot is under every day with events) and under it the agenda of the next 14 days from the
  chosen day, all-day events first, each with its time and title. Click a day to move the agenda to it; click an event to open
  it in the Calendar tab (Shale opens the tab if there is none) with its details. The pane has its own month and day, separate
  from the tab; the events and the sync are the same. It says so when no calendar is set up, when nothing is coming up and
  while the calendars load. See [[Calendar]].
- **Feeds can be in the right sidebar too.** Place the Feeds icon the same way. The pane is a compact companion of the Feeds
  tab: the accounts (a row of names when you have several), a feed picker (the home timeline first) and the posts as small
  rows: the author, how long ago, who boosted it, the first two lines of the text and the number of pictures (a post behind
  a content warning shows the warning, not the text). Click a post to open its conversation in a Feeds tab (the tab you are
  in if it is one, else a Feeds tab that shows this feed, else a new one); Back there returns to the feed. The pane has its
  own account, feed and scroll, separate from the tab; the accounts, the posts and the fetching are the same. Shale loads
  the first page of the feed the pane shows even when no Feeds tab is open; older posts and the check for new posts stay in
  the tab (only feeds that are open in a tab are checked), and the pane says so under the last post. It says so when there is
  no account, while the feeds load, and when nothing could be loaded or there is nothing yet. Reacting, writing and saving
  stay in the tab. See [[Feeds]].
- Drag the border between two views to change their heights.
- Drag the border of the left or of the right sidebar to change its width; Shale keeps the widths
  (`left_sidebar_width`, `right_sidebar_width`), and **Reset layout** goes back to the default widths.
- The layout is kept in the settings (`sidebars`, `show_left`, `show_right`), see
  [[Settings reference]].

# The app icon

The Shale icon (stacked slate plates) shows in the title bar and taskbar, and in the Dock on
macOS. On Linux with Wayland the taskbar takes it from a desktop file: `install.sh` in the download installs
the desktop file and the icons (see Start here).

# Dialogs

Every dialog is a window you can move by its title bar and resize from any edge or corner. In a
small window the content scrolls and the buttons stay inside the window.

#userguide #layout
